use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::oneshot;
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::Source;
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_as_lib::{TypstAsLibError, TypstEngine};
use typst_layout::PagedDocument;

use crate::config::Config;
use crate::resolver::{ConfinedResolver, MainSlot};

#[derive(Debug, Clone, Serialize)]
pub struct Diag {
    pub severity: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
}

#[derive(Debug)]
pub enum CompileOutcome {
    Ok {
        pdf: Vec<u8>,
        warnings: Vec<Diag>,
    },
    Failed {
        diags: Vec<Diag>,
    },
    /// Queue full — client should back off (429).
    Busy,
    /// Wall-clock timeout exceeded (500, generic).
    TimedOut,
    /// Panic or non-diagnostic engine error; detail is logged only (500).
    Internal,
}

struct Job {
    source: String,
    slug: String,
    reply: oneshot::Sender<CompileOutcome>,
}

pub struct CompilerPool {
    queue: crossbeam_channel::Sender<Job>,
    timeout: Duration,
}

impl CompilerPool {
    pub fn new(config: &Config) -> Result<Arc<CompilerPool>> {
        let workers = config.max_compiles_in_flight.max(1);
        let (tx, rx) = crossbeam_channel::bounded::<Job>(workers);
        for i in 0..workers {
            let rx = rx.clone();
            let config = config.clone();
            std::thread::Builder::new()
                .name(format!("typst-worker-{i}"))
                .spawn(move || worker_loop(&config, rx))?;
        }
        Ok(Arc::new(CompilerPool {
            queue: tx,
            timeout: config.compile_timeout,
        }))
    }

    pub async fn compile(&self, slug: String, source: String) -> CompileOutcome {
        let (reply_tx, reply_rx) = oneshot::channel();
        let job = Job {
            source,
            slug,
            reply: reply_tx,
        };
        if self.queue.try_send(job).is_err() {
            return CompileOutcome::Busy;
        }
        match tokio::time::timeout(self.timeout, reply_rx).await {
            Ok(Ok(outcome)) => outcome,
            // Worker dropped the reply (should not happen; treated as internal).
            Ok(Err(_)) => CompileOutcome::Internal,
            Err(_) => CompileOutcome::TimedOut,
        }
    }
}

/// One engine per worker thread: engines are not Sync, and a long-lived
/// engine keeps fonts/resolvers built once and comemo caches warm.
fn worker_loop(config: &Config, rx: crossbeam_channel::Receiver<Job>) {
    let slot = Arc::new(MainSlot::new());
    let resolver = match ConfinedResolver::new(config.templates_dir.clone(), config.allow_universe)
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("worker init failed, cannot canonicalize templates_dir: {e}");
            return;
        }
    };
    let mut builder = TypstEngine::builder()
        .search_fonts_with(
            TypstKitFontOptions::new().include_dirs(config.font_paths.iter().cloned()),
        )
        .add_file_resolver(SlotResolver(slot.clone()))
        .add_file_resolver(resolver);
    if config.allow_universe {
        builder = builder.with_package_file_resolver();
    }
    let engine = builder.build();

    while let Ok(job) = rx.recv() {
        let Job {
            source,
            slug,
            reply,
        } = job;
        let size = source.len();
        let start = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(|| compile_once(&engine, &slot, &source)))
            .unwrap_or_else(|panic| {
                let msg = panic_message(&panic);
                tracing::error!(slug, "compile panicked: {msg}");
                CompileOutcome::Internal
            });
        let outcome_label = match &outcome {
            CompileOutcome::Ok { .. } => "ok",
            CompileOutcome::Failed { .. } => "error",
            CompileOutcome::Internal => "panic",
            _ => "other",
        };
        tracing::info!(
            slug,
            size,
            duration_ms = start.elapsed().as_millis() as u64,
            outcome = outcome_label,
            "compile"
        );
        let _ = reply.send(outcome);
    }
}

/// Wrapper so the pool and the engine can share the slot (`add_file_resolver`
/// takes ownership).
struct SlotResolver(Arc<MainSlot>);

impl typst_as_lib::file_resolver::FileResolver for SlotResolver {
    fn resolve_binary(
        &self,
        id: typst::syntax::FileId,
    ) -> typst::diag::FileResult<std::borrow::Cow<'_, typst::foundations::Bytes>> {
        self.0.resolve_binary(id)
    }

    fn resolve_source(
        &self,
        id: typst::syntax::FileId,
    ) -> typst::diag::FileResult<std::borrow::Cow<'_, Source>> {
        self.0.resolve_source(id)
    }
}

fn compile_once(engine: &TypstEngine, slot: &MainSlot, source: &str) -> CompileOutcome {
    slot.set_source(source);
    let main = slot.source();
    let result = engine.compile::<_, PagedDocument>(MainSlot::file_id());
    let warnings = map_diags(&result.warnings, &main);
    match result.output {
        Ok(doc) => match typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()) {
            Ok(pdf) => CompileOutcome::Ok { pdf, warnings },
            Err(diags) => CompileOutcome::Failed {
                diags: with_warnings(map_diags(&diags, &main), warnings),
            },
        },
        Err(TypstAsLibError::TypstSource(diags)) => CompileOutcome::Failed {
            diags: with_warnings(map_diags(&diags, &main), warnings),
        },
        Err(e) => {
            tracing::error!("engine error: {e}");
            CompileOutcome::Internal
        }
    }
}

fn with_warnings(mut diags: Vec<Diag>, warnings: Vec<Diag>) -> Vec<Diag> {
    diags.extend(warnings);
    diags
}

/// Map Typst diagnostics to client diags. `line`/`col` are 1-based and only
/// set for spans inside the submitted main source.
fn map_diags(diags: &[SourceDiagnostic], main: &Source) -> Vec<Diag> {
    diags
        .iter()
        .map(|d| {
            let (line, col) = diag_range(d.span, main)
                .and_then(|range| {
                    let (line, col) = main.lines().byte_to_line_column(range.start)?;
                    Some((line + 1, col + 1))
                })
                .map(|(l, c)| (Some(l), Some(c)))
                .unwrap_or((None, None));
            Diag {
                severity: match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                },
                message: d.message.to_string(),
                line,
                col,
            }
        })
        .collect()
}

/// Byte range of a diagnostic span, but only within the submitted main source.
fn diag_range(span: typst::syntax::DiagSpan, main: &Source) -> Option<std::ops::Range<usize>> {
    use typst::syntax::DiagSpanKind;
    match span.get() {
        DiagSpanKind::Number { id, num, sub_range } if id == main.id() => {
            main.range(num, sub_range)
        }
        DiagSpanKind::Range { id, range } if id == main.id() => Some(range),
        _ => None,
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_config(templates: &Path) -> Config {
        let mut c = Config::from_toml("").unwrap();
        c.templates_dir = templates.to_path_buf();
        c.max_compiles_in_flight = 1;
        c
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("shared")).unwrap();
        std::fs::write(
            dir.path().join("shared/letter.typ"),
            "#let letter(body) = { body }",
        )
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn compiles_trivial_source_to_pdf() {
        let dir = fixture();
        let pool = CompilerPool::new(&test_config(dir.path())).unwrap();
        match pool.compile("t".into(), "= Hello".into()).await {
            CompileOutcome::Ok { pdf, .. } => assert_eq!(&pdf[..4], b"%PDF"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn maps_error_to_line_and_col() {
        let dir = fixture();
        let pool = CompilerPool::new(&test_config(dir.path())).unwrap();
        match pool.compile("t".into(), "= ok\n#undefinedvar".into()).await {
            CompileOutcome::Failed { diags } => {
                assert!(!diags.is_empty());
                assert_eq!(diags[0].severity, "error");
                assert_eq!(diags[0].line, Some(2));
                assert!(diags[0].col.is_some());
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn imports_shared_and_rejects_escapes() {
        let dir = fixture();
        let pool = CompilerPool::new(&test_config(dir.path())).unwrap();
        match pool
            .compile(
                "t".into(),
                "#import \"shared/letter.typ\": letter\nok".into(),
            )
            .await
        {
            CompileOutcome::Ok { .. } => {}
            other => panic!("shared import should work, got {other:?}"),
        }
        match pool
            .compile("t".into(), "#image(\"/etc/passwd\")".into())
            .await
        {
            CompileOutcome::Failed { diags } => {
                assert_eq!(diags[0].severity, "error");
            }
            other => panic!("expected Failed for /etc/passwd, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn all_seed_templates_compile() {
        let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let pool = CompilerPool::new(&test_config(&templates)).unwrap();
        let metas = crate::templates::list_templates(&templates);
        assert!(metas.len() >= 3, "expected seed templates, found {metas:?}");
        for meta in metas {
            let source = std::fs::read_to_string(templates.join(format!("{}.typ", meta.slug)))
                .unwrap();
            match pool.compile(meta.slug.clone(), source).await {
                CompileOutcome::Ok { pdf, warnings } => {
                    assert_eq!(&pdf[..4], b"%PDF", "{}", meta.slug);
                    assert!(
                        warnings.is_empty(),
                        "{}: unexpected warnings: {warnings:?}",
                        meta.slug
                    );
                }
                other => panic!("seed template {} must compile, got {other:?}", meta.slug),
            }
        }
    }

    #[tokio::test]
    async fn universe_disabled_gives_clear_diag() {
        let dir = fixture();
        let pool = CompilerPool::new(&test_config(dir.path())).unwrap();
        match pool
            .compile("t".into(), "#import \"@preview/example:0.1.0\": *".into())
            .await
        {
            CompileOutcome::Failed { diags } => {
                assert!(
                    diags[0].message.contains("allow_universe"),
                    "got: {}",
                    diags[0].message
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
