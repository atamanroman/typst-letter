use std::path::Path;

/// Router-level slug guard. Must pass before anything touches disk.
/// Allowed: `^[a-z0-9-]+$`, and never `shared`.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug != "shared"
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone)]
pub struct TemplateMeta {
    pub slug: String,
    pub title: String,
}

/// Discover templates: `{dir}/{slug}.typ` files, `shared/` excluded, sorted by slug.
pub fn list_templates(dir: &Path) -> Vec<TemplateMeta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("typ") {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !valid_slug(slug) {
            continue;
        }
        let title = std::fs::read_to_string(&path)
            .map(|src| extract_title(&src, slug))
            .unwrap_or_else(|_| slug.to_string());
        out.push(TemplateMeta {
            slug: slug.to_string(),
            title,
        });
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

/// Read a template's pristine source. Caller must have validated the slug.
pub fn read_template(dir: &Path, slug: &str) -> Option<String> {
    if !valid_slug(slug) {
        return None;
    }
    std::fs::read_to_string(dir.join(format!("{slug}.typ"))).ok()
}

/// First non-empty line with a leading `//` comment stripped; fallback: slug.
pub fn extract_title(src: &str, slug: &str) -> String {
    src.lines()
        .map(|l| l.trim().trim_start_matches("//").trim())
        .find(|l| !l.is_empty())
        .map(|l| l.to_string())
        .unwrap_or_else(|| slug.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_rules() {
        for good in ["business", "personal", "my-letter2", "a"] {
            assert!(valid_slug(good), "{good} should be valid");
        }
        for bad in ["", "shared", "a/b", "a.b", "..", "A", "über", "a_b", "a b"] {
            assert!(!valid_slug(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn title_extraction() {
        assert_eq!(extract_title("// Business letter\n#import", "x"), "Business letter");
        assert_eq!(extract_title("\n\n#import \"a\"\n", "x"), "#import \"a\"");
        assert_eq!(extract_title("", "fallback"), "fallback");
        assert_eq!(extract_title("\n  \n", "fallback"), "fallback");
    }

    #[test]
    fn discovery() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("shared")).unwrap();
        std::fs::write(root.join("shared/letter.typ"), "// nope").unwrap();
        std::fs::write(root.join("business.typ"), "// Business letter\nbody").unwrap();
        std::fs::write(root.join("personal.typ"), "").unwrap();
        std::fs::write(root.join("README.md"), "not a template").unwrap();
        std::fs::write(root.join("Bad_Name.typ"), "skipped, invalid slug").unwrap();

        let list = list_templates(root);
        let slugs: Vec<_> = list.iter().map(|t| t.slug.as_str()).collect();
        assert_eq!(slugs, ["business", "personal"]);
        assert_eq!(list[0].title, "Business letter");
        assert_eq!(list[1].title, "personal"); // empty file falls back to slug
    }

    #[test]
    fn read_template_guards_slug() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.typ"), "hi").unwrap();
        assert_eq!(read_template(dir.path(), "ok").unwrap(), "hi");
        assert!(read_template(dir.path(), "missing").is_none());
        assert!(read_template(dir.path(), "../ok").is_none());
        assert!(read_template(dir.path(), "shared").is_none());
    }
}
