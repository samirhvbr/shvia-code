//! Contexto do projeto: raiz (git), branch, arquivo de instruções (ANNA.md /
//! CLAUDE.md / AGENTS.md) e o system prompt da Anna.

use std::path::{Path, PathBuf};

pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub git_branch: Option<String>,
    pub context: Option<String>,
}

impl Project {
    pub fn detect() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = git_root(&cwd).unwrap_or(cwd);
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "projeto".into());
        let git_branch = current_branch(&root);
        let context = ["ANNA.md", "CLAUDE.md", "AGENTS.md"]
            .iter()
            .map(|f| root.join(f))
            .find(|p| p.exists())
            .and_then(|p| std::fs::read_to_string(p).ok());
        Self { root, name, git_branch, context }
    }

    pub fn system_prompt(&self) -> String {
        let mut s = String::from(
            "Você é a Anna, agente de código da Blue3, operando no terminal do dev via SHVIA-CODE. \
             Use as ferramentas (read_file, list_dir, grep, glob) para INSPECIONAR o projeto antes de \
             afirmar qualquer coisa — não invente caminhos nem conteúdo. Prefira poucas leituras \
             certeiras. Responda em pt-BR, direto ao ponto, citando arquivo:linha quando útil.",
        );
        s.push_str(&format!("\n\nProjeto: {} (raiz: {}).", self.name, self.root.display()));
        if let Some(ctx) = &self.context {
            let trimmed: String = ctx.chars().take(6000).collect();
            s.push_str("\n\nInstruções do projeto (arquivo de contexto):\n");
            s.push_str(&trimmed);
        }
        s
    }

    pub fn header(&self, model: &str, server: &str) -> String {
        let git = self.git_branch.clone().unwrap_or_else(|| "sem git".into());
        format!(
            "ANNA · ShvIA Code — projeto {} ({}) · {} @ {}",
            self.name, git, model, server
        )
    }
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start.to_path_buf());
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        cur = dir.parent().map(Path::to_path_buf);
    }
    None
}

fn current_branch(root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(root.join(".git/HEAD")).ok()?;
    head.strip_prefix("ref: refs/heads/").map(|b| b.trim().to_string())
}
