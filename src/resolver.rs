//! Security boundary: the only filesystem access path for submitted Typst code.
//!
//! Threat model: the browser submits arbitrary Typst. Typst itself has no
//! network or shell access; with reads confined to the templates tree, the
//! worst case for a hostile paste is reading files under `templates/`.
//! Acceptable for a personal / VPN-only instance.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::RwLock;

use ecow::eco_format;
use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst_as_lib::file_resolver::FileResolver;

/// Holds the editable main source submitted by the browser. Reusing one
/// `Source` (same `FileId`) across compiles keeps Typst's incremental
/// compilation effective.
pub struct MainSlot {
    source: RwLock<Source>,
}

pub const MAIN_PATH: &str = "/__main__.typ";

impl MainSlot {
    pub fn new() -> Self {
        Self {
            source: RwLock::new(Source::new(Self::file_id(), String::new())),
        }
    }

    pub fn file_id() -> FileId {
        RootedPath::new(VirtualRoot::Project, VirtualPath::new(MAIN_PATH).unwrap()).intern()
    }

    pub fn set_source(&self, text: &str) {
        self.source.write().unwrap().replace(text);
    }

    /// Snapshot of the current source, for mapping diagnostic spans.
    pub fn source(&self) -> Source {
        self.source.read().unwrap().clone()
    }
}

impl FileResolver for MainSlot {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<'_, Bytes>> {
        Err(FileError::NotFound(id.vpath().get_with_slash().into()))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<'_, Source>> {
        if id == Self::file_id() {
            Ok(Cow::Owned(self.source()))
        } else {
            Err(FileError::NotFound(id.vpath().get_with_slash().into()))
        }
    }
}

/// Resolves project files strictly within the templates root. Read-only.
pub struct ConfinedResolver {
    /// Canonicalized templates root.
    root: PathBuf,
    allow_universe: bool,
}

impl ConfinedResolver {
    pub fn new(root: PathBuf, allow_universe: bool) -> std::io::Result<Self> {
        Ok(Self {
            root: root.canonicalize()?,
            allow_universe,
        })
    }

    fn resolve_path(&self, id: FileId) -> FileResult<PathBuf> {
        match id.root() {
            VirtualRoot::Package(pkg) => {
                // Package ids are handled by the package resolver when
                // allow_universe is on; this resolver only reports the
                // disabled case with a clear diagnostic.
                let msg = if self.allow_universe {
                    eco_format!("package {pkg} not available")
                } else {
                    eco_format!(
                        "universe packages are disabled (allow_universe = false); \
                         vendor shared code into templates/shared/ instead"
                    )
                };
                return Err(FileError::Other(Some(msg)));
            }
            VirtualRoot::Project => {}
        }
        // `realize` confines lexically (rejects `..` escapes and absolute
        // paths outside the root) ...
        let path = id
            .vpath()
            .realize(&self.root)
            .map_err(|_| FileError::AccessDenied)?;
        // ... and canonicalize + prefix check also catches symlink escapes.
        let canonical = path
            .canonicalize()
            .map_err(|e| FileError::from_io(e, &path))?;
        if !canonical.starts_with(&self.root) {
            return Err(FileError::AccessDenied);
        }
        Ok(canonical)
    }
}

impl FileResolver for ConfinedResolver {
    fn resolve_binary(&self, id: FileId) -> FileResult<Cow<'_, Bytes>> {
        let path = self.resolve_path(id)?;
        let content = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
        Ok(Cow::Owned(Bytes::new(content)))
    }

    fn resolve_source(&self, id: FileId) -> FileResult<Cow<'_, Source>> {
        let path = self.resolve_path(id)?;
        let bytes = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
        let text = String::from_utf8(bytes).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Cow::Owned(Source::new(id, text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_id(path: &str) -> FileId {
        RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).unwrap()).intern()
    }

    fn fixture() -> (tempfile::TempDir, ConfinedResolver, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("templates");
        std::fs::create_dir_all(root.join("shared")).unwrap();
        std::fs::write(root.join("shared/letter.typ"), "#let letter = 1").unwrap();
        // a secret outside the root that must never be readable
        let secret = dir.path().join("secret.typ");
        std::fs::write(&secret, "top secret").unwrap();
        let resolver = ConfinedResolver::new(root, false).unwrap();
        (dir, resolver, secret)
    }

    #[test]
    fn resolves_inside_root() {
        let (_dir, resolver, _) = fixture();
        let src = resolver
            .resolve_source(project_id("shared/letter.typ"))
            .unwrap();
        assert_eq!(src.text(), "#let letter = 1");
        assert!(resolver
            .resolve_binary(project_id("shared/letter.typ"))
            .is_ok());
    }

    #[test]
    fn rejects_escape_via_dotdot() {
        // typst 0.15 rejects escaping paths at VirtualPath construction,
        // before a FileId can even exist.
        assert!(VirtualPath::new("../secret.typ").is_err());
        assert!(VirtualPath::new("shared/../../secret.typ").is_err());
    }

    #[test]
    fn rejects_absolute_system_path() {
        let (_dir, resolver, _) = fixture();
        // VirtualPath is rooted, so "/etc/passwd" means "{root}/etc/passwd";
        // it must not resolve to the real /etc/passwd.
        assert!(resolver.resolve_binary(project_id("/etc/passwd")).is_err());
    }

    #[test]
    fn rejects_symlink_escape() {
        let (dir, resolver, secret) = fixture();
        let link = dir.path().join("templates/sneaky.typ");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let err = resolver
            .resolve_source(project_id("sneaky.typ"))
            .unwrap_err();
        assert!(matches!(err, FileError::AccessDenied), "got {err:?}");
    }

    #[test]
    fn package_disabled_message() {
        let (_dir, resolver, _) = fixture();
        let pkg: typst::syntax::package::PackageSpec = "@preview/example:0.1.0".parse().unwrap();
        let id = RootedPath::new(
            VirtualRoot::Package(pkg),
            VirtualPath::new("lib.typ").unwrap(),
        )
        .intern();
        let err = resolver.resolve_source(id).unwrap_err();
        assert!(err.to_string().contains("allow_universe"), "got {err}");
    }

    #[test]
    fn main_slot_swaps_source() {
        let slot = MainSlot::new();
        slot.set_source("= Hello");
        assert_eq!(slot.source().text(), "= Hello");
        slot.set_source("= Bye");
        assert_eq!(slot.source().text(), "= Bye");
        let resolved = slot.resolve_source(MainSlot::file_id()).unwrap();
        assert_eq!(resolved.text(), "= Bye");
        assert!(slot.resolve_source(project_id("other.typ")).is_err());
    }
}
