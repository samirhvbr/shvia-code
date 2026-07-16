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

    pub fn system_prompt(&self, host_mode: bool) -> String {
        let base = if host_mode {
            // Modo host (aba Agente do SShvTerm): a Anna PROPÕE comandos de shell;
            // quem executa/aprova é o host. Só a ferramenta `bash` é oferecida.
            "Você é a Anna, agente de código da Blue3, operando pela aba Agente do SShvTerm. \
             Você PROPÕE comandos de shell (ferramenta `bash`) que o usuário roda no host dele \
             (terminal visível ou SSH) — UM comando por vez, sempre explicando o porquê em uma \
             frase. Você NUNCA executa: quem aprova e roda é o usuário, com a política dele. \
             Prefira comandos de leitura (ls, cat, grep, git status/diff) antes de propor qualquer \
             mudança. Responda em pt-BR, direto ao ponto."
        } else {
            "Você é a Anna, agente de código da Blue3, operando no terminal do dev via SHVIA-CODE.\n\n\
             Ferramentas: leitura (read_file, list_dir, grep, glob), escrita (edit_file, write_file), \
             execução (bash) e git (git_status, git_diff, git_commit). SEMPRE inspecione o projeto com \
             as ferramentas antes de afirmar ou alterar qualquer coisa — não invente caminhos nem \
             conteúdo; prefira poucas leituras certeiras.\n\n\
             Criar ou alterar arquivo é SÓ chamando a ferramenta edit_file/write_file — é ela que \
             mostra o diff e pede a aprovação do dev. NUNCA cole o conteúdo do arquivo no chat pedindo \
             \"posso criar/escrever?\": texto no chat não cria nada. Quando o dev pedir para criar, \
             escrever, gerar ou modificar algo — ou aprovar (\"sim\", \"pode\", \"faça\") uma proposta \
             sua — EMITA a chamada da ferramenta no MESMO turno, sem repetir o conteúdo em prosa. Faça \
             a MENOR mudança que resolve. \"Proponha, não force\" vale para a chamada da ferramenta \
             (que já passa pela aprovação do dev antes de efetivar), não para narrar em texto: se uma \
             escrita/execução for recusada, siga sem insistir. Rode testes/checagens com bash quando \
             fizer sentido. Só commite (git_commit) se o dev pedir.\n\n\
             Responda em pt-BR, direto ao ponto, citando arquivo:linha quando útil."
        };
        let mut s = String::from(base);
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
