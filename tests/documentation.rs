use std::fs;
use std::path::{Path, PathBuf};

fn markdown_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            markdown_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
    Ok(())
}

#[test]
fn current_documentation_and_local_links_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "docs/architecture.md",
        "docs/configuration.md",
        "docs/product.md",
        "docs/ux.md",
        "docs/support.md",
        "docs/install.md",
        "docs/security.md",
        "docs/troubleshooting.md",
        "docs/release-checklist.md",
        "docs/cli/tale.1",
        "completions/tale.bash",
        "completions/_tale",
        "completions/tale.fish",
        "LICENSE",
        "NOTICE",
        "deny.toml",
        "release/README.md",
        "tests/acceptance/journeys.md",
    ] {
        assert!(root.join(relative).is_file(), "missing {relative}");
    }

    let readme = fs::read_to_string(root.join("README.md"));
    assert!(readme.is_ok());
    if let Ok(readme) = readme {
        for link in [
            "docs/support.md",
            "docs/install.md",
            "docs/security.md",
            "docs/troubleshooting.md",
            "docs/release-checklist.md",
            "docs/cli/tale.1",
        ] {
            assert!(readme.contains(link), "README is missing {link}");
        }
    }

    let support = fs::read_to_string(root.join("docs/support.md"));
    assert!(support.is_ok());
    if let Ok(support) = support {
        assert!(support.contains("There are no Supported 1.0 platform rows yet"));
        assert!(support.contains("x86_64-unknown-linux-gnu"));
        assert!(support.contains("x86_64-pc-windows-msvc"));
    }
}

#[test]
fn local_markdown_links_resolve_without_network_access() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    assert!(markdown_files(&root.join("docs"), &mut files).is_ok());
    assert!(markdown_files(&root.join("tests/acceptance"), &mut files).is_ok());
    assert!(markdown_files(&root.join("release"), &mut files).is_ok());
    for file in files {
        let source = fs::read_to_string(&file);
        assert!(source.is_ok(), "could not read {}", file.display());
        if let Ok(source) = source {
            for section in source.split("](").skip(1) {
                let Some(target) = section.split(')').next() else {
                    continue;
                };
                let target = target.trim().trim_matches('<');
                let target = target.trim_end_matches('>');
                if target.is_empty()
                    || target.starts_with('#')
                    || target.starts_with("http://")
                    || target.starts_with("https://")
                    || target.starts_with("mailto:")
                {
                    continue;
                }
                let target = target.split(['#', '?']).next().map_or("", |value| value);
                if target.is_empty() {
                    continue;
                }
                let resolved = file
                    .parent()
                    .map_or_else(|| root.join(target), |parent| parent.join(target));
                assert!(
                    resolved.exists(),
                    "broken local link {target} in {}",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn semantic_theme_decision_and_token_ledger_are_present() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "docs/decisions/0005-semantic-theme-system.md",
        "docs/design/theme-token-ledger.md",
    ] {
        let contents = fs::read_to_string(root.join(relative));
        assert!(contents.is_ok(), "missing {relative}");
        if let Ok(contents) = contents {
            assert!(contents.contains("tailscale-dark"));
            assert!(contents.contains("no-color") || contents.contains("no color"));
        }
    }
}

/// Shared components exist so a second copy cannot drift from the first. A view
/// building its own bordered box or its own table is how `┌inspector─` ended up
/// beside `┌ devices ─`, and how two width solvers ended up in one crate.
#[test]
fn views_do_not_build_their_own_panels_or_tables() {
    let mut offenders = Vec::new();
    let Ok(entries) = std::fs::read_dir("src/ui/views") else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if source.contains("Block::default()") {
            offenders.push(format!("{} builds its own border", path.display()));
        }
        if source.contains("Borders::ALL") {
            offenders.push(format!("{} sets its own borders", path.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "use components::panel instead:\n{}",
        offenders.join("\n")
    );
}
