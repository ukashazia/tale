use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::Visit;
use tale::admin::auth::SecretValue;
use tale::domain::secret_result::{SecretBuffer, SecretMetadata, SecretResult};

#[derive(Default)]
struct ForbiddenRustVisitor {
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for ForbiddenRustVisitor {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        let is_command_constructor = match node.func.as_ref() {
            syn::Expr::Path(path) => {
                path.path
                    .segments
                    .iter()
                    .any(|segment| segment.ident == "Command")
                    && path
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "new")
            }
            _ => false,
        };
        if is_command_constructor
            && node
                .args
                .first()
                .and_then(string_literal)
                .is_some_and(|value| {
                    let executable = value
                        .rsplit(['/', '\\'])
                        .next()
                        .map_or(value.as_str(), |part| part);
                    matches!(
                        executable.to_ascii_lowercase().as_str(),
                        "sh" | "sh.exe"
                            | "bash"
                            | "bash.exe"
                            | "zsh"
                            | "zsh.exe"
                            | "dash"
                            | "dash.exe"
                            | "fish"
                            | "fish.exe"
                            | "cmd"
                            | "cmd.exe"
                            | "powershell"
                            | "powershell.exe"
                            | "pwsh"
                            | "pwsh.exe"
                            | "sudo"
                    )
                })
        {
            self.violations
                .push("shell or sudo executable passed to Command::new".to_owned());
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        if method == "unwrap" || method == "expect" {
            self.violations.push(format!("method {method}"));
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
        let name = node
            .mac
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string());
        if matches!(name.as_str(), "panic" | "todo" | "unimplemented") {
            self.violations.push(format!("macro {name}"));
        }
        if matches!(
            name.as_str(),
            "println" | "eprintln" | "debug" | "trace" | "warn" | "info"
        ) {
            let tokens = node.mac.tokens.to_string().to_ascii_lowercase();
            if tokens.contains(":?")
                && ["config", "credential", "token", "policy", "audit", "flow"]
                    .iter()
                    .any(|field| tokens.contains(field))
            {
                self.violations
                    .push("broad sensitive debug output".to_owned());
            }
            if ["authorization", "access_token", "client_secret"]
                .iter()
                .any(|field| tokens.contains(field))
            {
                self.violations
                    .push("token or Authorization field in logging macro".to_owned());
            }
        }
        syn::visit::visit_expr_macro(self, node);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.violations.push("unsafe block".to_owned());
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.unsafety.is_some() {
            self.violations.push("unsafe function".to_owned());
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if node.unsafety.is_some() {
            self.violations.push("unsafe impl".to_owned());
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if node.unsafety.is_some() {
            self.violations.push("unsafe trait".to_owned());
        }
        syn::visit::visit_item_trait(self, node);
    }
}

fn string_literal(expression: &syn::Expr) -> Option<String> {
    match expression {
        syn::Expr::Lit(literal) => match &literal.lit {
            syn::Lit::Str(value) => Some(value.value()),
            _ => None,
        },
        _ => None,
    }
}

fn rust_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

#[test]
fn repository_rust_has_no_forbidden_executable_patterns() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    assert!(rust_files(&root.join("src"), &mut files).is_ok());
    assert!(rust_files(&root.join("tests"), &mut files).is_ok());
    assert!(rust_files(&root.join("benches"), &mut files).is_ok());
    let mut violations = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path);
        assert!(source.is_ok(), "could not read {}", path.display());
        if let Ok(source) = source {
            let parsed = syn::parse_file(&source);
            assert!(parsed.is_ok(), "could not parse {}", path.display());
            if let Ok(parsed) = parsed {
                let mut visitor = ForbiddenRustVisitor::default();
                visitor.visit_file(&parsed);
                violations.extend(
                    visitor
                        .violations
                        .into_iter()
                        .map(|violation| format!("{}: {violation}", path.display())),
                );
            }
        }
    }
    assert!(
        violations.is_empty(),
        "forbidden Rust patterns: {violations:?}"
    );
}

#[test]
fn locked_dependencies_use_the_committed_license_policy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = fs::read_to_string(root.join("deny.toml"));
    assert!(policy.is_ok());
    if let Ok(policy) = policy {
        for license in ["Apache-2.0", "BSD-3-Clause", "ISC", "MIT"] {
            assert!(policy.contains(license));
        }
        assert!(policy.contains("unknown-registry = \"deny\""));
        assert!(policy.contains("unknown-git = \"deny\""));
    }
    let lock = fs::read_to_string(root.join("Cargo.lock"));
    assert!(lock.is_ok());
    if let Ok(lock) = lock {
        assert!(!lock.contains("git+"));
        assert!(!lock.contains("registry+https://example.invalid"));
    }
}

#[test]
fn secret_canaries_are_redacted_and_view_once_results_close() {
    let canary = "fictional-secret-canary";
    let credential = SecretValue::new(canary);
    assert!(!format!("{credential:?}").contains(canary));

    let metadata = SecretMetadata {
        result_id: 1,
        credential_id: Some("fictional-id".to_owned()),
        credential_type: "auth key".to_owned(),
        description: Some("one-time result".to_owned()),
        created_at: 1,
        expires_at: None,
        warning: "copy once".to_owned(),
    };
    let mut result = SecretResult::new(metadata, SecretBuffer::new(canary));
    assert!(!format!("{result:?}").contains(canary));
    result.close();
    assert!(result.is_closed());
}

#[test]
fn post_v1_obsolete_runtime_and_documentation_paths_are_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    assert!(rust_files(&root.join("src"), &mut files).is_ok());
    let forbidden_rust = [
        "PreferenceClient",
        "route_stack",
        "ActionPicker",
        "CopyPicker",
        "CommandPalette",
        "FilterEditor",
    ];
    let mut violations = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path);
        assert!(source.is_ok(), "could not read {}", path.display());
        if let Ok(source) = source {
            for forbidden in forbidden_rust {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains {forbidden}", path.display()));
                }
            }
        }
    }

    for relative in [
        "README.md",
        "docs/product.md",
        "docs/architecture.md",
        "docs/ux.md",
        "docs/configuration.md",
        "docs/support.md",
        "docs/security.md",
        "docs/install.md",
        "docs/troubleshooting.md",
        "docs/release-checklist.md",
        "docs/cli/tale.1",
        "completions/tale.bash",
        "completions/tale.fish",
        "completions/_tale",
    ] {
        let path = root.join(relative);
        let source = fs::read_to_string(&path);
        assert!(source.is_ok(), "could not read {}", path.display());
        if let Ok(source) = source {
            for forbidden in [
                "local status polling",
                "tailscale status --json observation",
                "preferences-only HTTP transport",
                "route_stack",
                "q-as-back",
                "Esc-as-route-back",
                "centered command palette",
                "centered filter editor",
                "CLI fallback for LocalAPI observation",
            ] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains {forbidden}", path.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "obsolete post-v1 paths remain: {violations:?}"
    );
}
