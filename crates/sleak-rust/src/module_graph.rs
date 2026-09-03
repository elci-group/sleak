use camino::{Utf8Path, Utf8PathBuf};
use sleak_core::{EdgeKind, NodeId, NodeKind, Result, SleakError};
use sleak_ir::{Edge, Node};
use std::collections::{BTreeMap, BTreeSet};

pub struct ModuleFile {
    pub node_id: NodeId,
    pub module_path: Vec<String>,
    pub file_path: Utf8PathBuf,
}

impl ModuleFile {
    pub fn to_node(&self) -> Node {
        Node {
            id: self.node_id.clone(),
            kind: NodeKind::Module,
            label: self.module_path.join("::"),
        }
    }
}

pub fn module_node_id(crate_name: &str, module_path: &[String]) -> NodeId {
    if module_path.is_empty() {
        NodeId::new(format!("module:{crate_name}"))
    } else {
        NodeId::new(format!("module:{crate_name}::{}", module_path.join("::")))
    }
}

/// Walks `src_root` for `.rs` files and maps each one to a module path
/// using Rust's own file-to-module convention (`foo/bar.rs` and
/// `foo/bar/mod.rs` both mean `foo::bar`; `lib.rs`/`main.rs` is the
/// crate root). This is a filesystem-convention heuristic, not a
/// resolution of the crate's actual `mod` declarations — a file with no
/// corresponding `mod` statement anywhere would still be listed. That
/// tradeoff is deliberate: it needs no cross-file state to compute and
/// matches the layout of every crate in this workspace.
pub fn discover_module_files(crate_name: &str, src_root: &Utf8Path) -> Result<Vec<ModuleFile>> {
    let mut files = Vec::new();
    if src_root.is_dir() {
        collect_rs_files(src_root, &mut files)?;
    }
    files.sort();

    let mut out = Vec::with_capacity(files.len());
    for file_path in files {
        let rel = file_path
            .strip_prefix(src_root)
            .expect("file was found under src_root");
        let mut segments: Vec<String> = rel
            .as_str()
            .trim_end_matches(".rs")
            .split('/')
            .map(str::to_string)
            .collect();
        if segments.last().map(String::as_str) == Some("mod") {
            segments.pop();
        }
        let module_path = match segments.as_slice() {
            [only] if only == "lib" || only == "main" => Vec::new(),
            _ => segments,
        };
        let node_id = module_node_id(crate_name, &module_path);
        out.push(ModuleFile {
            node_id,
            module_path,
            file_path,
        });
    }
    Ok(out)
}

fn collect_rs_files(dir: &Utf8Path, out: &mut Vec<Utf8PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|source| SleakError::Io {
        path: dir.as_std_path().to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| SleakError::Io {
            path: dir.as_std_path().to_path_buf(),
            source,
        })?;
        let path = Utf8PathBuf::try_from(entry.path()).map_err(|e| SleakError::ParseFailure {
            file: e.into_path_buf(),
            reason: "path is not valid UTF-8".to_string(),
        })?;
        let is_dir = entry
            .file_type()
            .map_err(|source| SleakError::Io {
                path: path.clone().into_std_path_buf(),
                source,
            })?
            .is_dir();
        if is_dir {
            collect_rs_files(&path, out)?;
        } else if path.extension() == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Every raw (unresolved) segment path named by a `use` item in a
/// parsed file — group and glob imports are expanded, e.g.
/// `use a::{b, c::*}` yields `[["a","b"], ["a","c"]]`.
fn extract_use_targets(file: &syn::File) -> Vec<Vec<String>> {
    let mut targets = Vec::new();
    for item in &file.items {
        if let syn::Item::Use(item_use) = item {
            let mut prefix = Vec::new();
            walk_use_tree(&item_use.tree, &mut prefix, &mut targets);
        }
    }
    targets
}

fn walk_use_tree(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            walk_use_tree(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let mut full = prefix.clone();
            full.push(n.ident.to_string());
            out.push(full);
        }
        syn::UseTree::Rename(r) => {
            let mut full = prefix.clone();
            full.push(r.ident.to_string());
            out.push(full);
        }
        syn::UseTree::Glob(_) => {
            out.push(prefix.clone());
        }
        syn::UseTree::Group(g) => {
            for t in &g.items {
                walk_use_tree(t, prefix, out);
            }
        }
    }
}

/// Resolves one raw `use` segment path to a known module node.
///
/// Only `crate`/`self`/`super`-relative paths and paths whose leading
/// segment names another workspace crate are resolved — anything else
/// (`std::...`, an external crate, an unqualified 2015-style path) is
/// left ungraphed rather than guessed at (directive §2.3: evidence
/// before judgement). Once a target crate is identified, the longest
/// prefix of the remaining segments that matches a known module in that
/// crate is used — this lets `use crate::foo::SomeType` resolve to the
/// `foo` module even though `SomeType` itself isn't a module.
fn resolve_use(
    raw: &[String],
    current_crate: &str,
    current_module_path: &[String],
    known_crate_names: &BTreeSet<String>,
    module_registry: &BTreeMap<String, BTreeSet<Vec<String>>>,
) -> Option<NodeId> {
    let head = raw.first()?;
    let (target_crate, target_path): (String, Vec<String>) = if head == "crate" {
        (current_crate.to_string(), raw[1..].to_vec())
    } else if head == "self" {
        let mut p = current_module_path.to_vec();
        p.extend_from_slice(&raw[1..]);
        (current_crate.to_string(), p)
    } else if head == "super" {
        let mut depth = 0usize;
        while depth < raw.len() && raw[depth] == "super" {
            depth += 1;
        }
        if depth > current_module_path.len() {
            return None;
        }
        let mut p = current_module_path[..current_module_path.len() - depth].to_vec();
        p.extend_from_slice(&raw[depth..]);
        (current_crate.to_string(), p)
    } else if known_crate_names.contains(head) {
        (head.clone(), raw[1..].to_vec())
    } else {
        return None;
    };

    let known = module_registry.get(&target_crate)?;
    for len in (0..=target_path.len()).rev() {
        let candidate = &target_path[..len];
        if known.contains(candidate) {
            return Some(module_node_id(&target_crate, candidate));
        }
    }
    None
}

/// Parses `file` and returns the `Imports` edges resolvable from its
/// top-level `use` statements. Only top-level items are inspected —
/// `use` statements inside an inline `mod { ... }` block (e.g. a
/// `#[cfg(test)] mod tests { use super::*; ... }`) are not descended
/// into; that scope is almost always self-referential in practice and
/// tracking nested inline-module paths is deferred until real signal is
/// lost by skipping it.
pub fn import_edges(
    file: &ModuleFile,
    current_crate: &str,
    known_crate_names: &BTreeSet<String>,
    module_registry: &BTreeMap<String, BTreeSet<Vec<String>>>,
) -> Result<Vec<Edge>> {
    let contents = std::fs::read_to_string(&file.file_path).map_err(|source| SleakError::Io {
        path: file.file_path.clone().into_std_path_buf(),
        source,
    })?;
    let parsed = syn::parse_file(&contents).map_err(|e| SleakError::ParseFailure {
        file: file.file_path.clone().into_std_path_buf(),
        reason: e.to_string(),
    })?;

    let mut edges = Vec::new();
    for raw in extract_use_targets(&parsed) {
        if let Some(target) = resolve_use(
            &raw,
            current_crate,
            &file.module_path,
            known_crate_names,
            module_registry,
        ) {
            if target != file.node_id {
                edges.push(Edge {
                    from: file.node_id.clone(),
                    to: target,
                    kind: EdgeKind::Imports,
                });
            }
        }
    }
    Ok(edges)
}
