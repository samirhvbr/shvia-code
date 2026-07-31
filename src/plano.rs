//! Os CONTRATOS entre as fases do orquestrador — a peça que sustenta o resto.
//!
//! Os papéis não compartilham contexto, compartilham artefato: o arquiteto
//! devolve um `Plano`, o revisor devolve um `Veredito`, e é só isso que
//! atravessa a fronteira. Por isso o schema é validado AQUI: se o modelo derivar
//! de formato, a fase falha alto (com a mensagem indo de volta pra ele numa
//! segunda tentativa) em vez de degradar em silêncio lá no executor.
//!
//! Regra dura do `o_que`/`o_que_fazer`: **o executor nunca viu o raciocínio de
//! quem escreveu**. Instrução que só faz sentido pra quem leu o thinking do
//! arquiteto é instrução quebrada — daí a validação de tamanho mínimo.

use serde::{Deserialize, Serialize};

// ── Plano (arquiteto → executor) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plano {
    pub objetivo: String,
    pub passos: Vec<Passo>,
    /// O que NÃO tocar. Sem isto o executor "melhora" o que está em volta.
    #[serde(default)]
    pub fora_de_escopo: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Passo {
    pub id: String,
    pub arquivo: String,
    /// `editar` | `criar` | `deletar`.
    pub acao: String,
    /// Instrução AUTOSSUFICIENTE (ver nota do módulo).
    pub o_que: String,
    /// Critérios verificáveis — é a régua do revisor. Sem régua fixa o loop de
    /// revisão não converge (sempre dá pra "melhorar" alguma coisa).
    #[serde(default)]
    pub aceite: Vec<String>,
}

const ACOES: &[&str] = &["editar", "criar", "deletar"];
const MIN_O_QUE: usize = 20;

impl Plano {
    pub fn validar(&self) -> Result<(), String> {
        if self.objetivo.trim().is_empty() {
            return Err("plano sem 'objetivo'".into());
        }
        if self.passos.is_empty() {
            return Err("plano sem passos".into());
        }
        let mut ids = std::collections::HashSet::new();
        for p in &self.passos {
            if p.id.trim().is_empty() {
                return Err("passo sem 'id'".into());
            }
            if !ids.insert(p.id.as_str()) {
                return Err(format!("id de passo repetido: '{}'", p.id));
            }
            if p.arquivo.trim().is_empty() {
                return Err(format!("passo '{}' sem 'arquivo'", p.id));
            }
            if !ACOES.contains(&p.acao.as_str()) {
                return Err(format!(
                    "passo '{}': acao '{}' inválida (use editar|criar|deletar)",
                    p.id, p.acao
                ));
            }
            if p.o_que.trim().chars().count() < MIN_O_QUE {
                return Err(format!(
                    "passo '{}': 'o_que' curto demais — o executor NÃO vê seu raciocínio, \
                     a instrução tem que se sustentar sozinha",
                    p.id
                ));
            }
        }
        Ok(())
    }

    /// Resumo de uma linha por passo, pro gate humano.
    pub fn resumo(&self) -> String {
        let mut s = format!("objetivo: {}\n", self.objetivo.trim());
        for p in &self.passos {
            s.push_str(&format!("  [{}] {} {}\n", p.id, p.acao, p.arquivo));
            s.push_str(&format!("       {}\n", uma_linha(&p.o_que, 100)));
            for a in &p.aceite {
                s.push_str(&format!("       ✓ {}\n", uma_linha(a, 100)));
            }
        }
        if !self.fora_de_escopo.is_empty() {
            s.push_str(&format!("  fora de escopo: {}\n", self.fora_de_escopo.join("; ")));
        }
        s
    }
}

// ── Veredito (revisor → executor, ou → dev) ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Veredito {
    pub aprovado: bool,
    #[serde(default)]
    pub achados: Vec<Achado>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Achado {
    /// Amarra o achado ao plano — sem isto não dá pra rotear nem deduplicar.
    #[serde(default)]
    pub passo_id: String,
    /// `desvio` (diff ≠ passo) | `escopo` (diff além do plano) | `plano_errado`
    /// (diff fiel, plano furado). String crua: classe desconhecida vira `desvio`
    /// no roteamento em vez de derrubar o parse.
    pub classe: String,
    #[serde(default)]
    pub aceite_violado: String,
    #[serde(default)]
    pub onde: String,
    /// Instrução acionável — mesma regra dura do `o_que` do plano.
    pub o_que_fazer: String,
}

impl Achado {
    /// Falha do PLANO, não do executor: mandar de volta pro executor só gera
    /// ping-pong. Para e devolve pro dev decidir.
    pub fn plano_errado(&self) -> bool {
        self.classe.trim().eq_ignore_ascii_case("plano_errado")
    }

    /// Chave de deduplicação: o mesmo achado voltando depois de uma tentativa de
    /// correção significa que insistir não resolve.
    pub fn chave(&self) -> String {
        format!("{}|{}", self.passo_id.trim(), self.aceite_violado.trim())
    }
}

impl Veredito {
    pub fn validar(&self) -> Result<(), String> {
        if !self.aprovado && self.achados.is_empty() {
            return Err("veredito reprovado sem nenhum achado — não dá pra corrigir nada".into());
        }
        for a in &self.achados {
            if a.o_que_fazer.trim().chars().count() < MIN_O_QUE {
                return Err(format!(
                    "achado do passo '{}': 'o_que_fazer' curto demais — o executor \
                     não viu seu raciocínio",
                    a.passo_id
                ));
            }
        }
        Ok(())
    }

    pub fn resumo(&self) -> String {
        if self.aprovado {
            return "aprovado — o diff entrega o que o plano prometeu.".into();
        }
        let mut s = format!("reprovado ({} achado(s)):\n", self.achados.len());
        for a in &self.achados {
            let onde = if a.onde.trim().is_empty() { String::new() } else { format!(" ({})", a.onde) };
            s.push_str(&format!("  [{}] {}{}\n", a.passo_id, a.classe, onde));
            if !a.aceite_violado.trim().is_empty() {
                s.push_str(&format!("       aceite violado: {}\n", uma_linha(&a.aceite_violado, 100)));
            }
            s.push_str(&format!("       → {}\n", uma_linha(&a.o_que_fazer, 100)));
        }
        s
    }
}

// ── Extração do JSON da resposta do modelo ──────────────────────────────────

/// Isola o objeto JSON de uma resposta que pode vir com cerca ```json e prosa em
/// volta. Vai do primeiro `{` até a chave que o fecha, contando profundidade e
/// **ignorando chaves dentro de string** — um `"o_que": "troque o { por ["`
/// quebraria a busca ingênua pelo último `}`.
pub fn extrai_objeto(texto: &str) -> Option<&str> {
    let bytes = texto.as_bytes();
    let inicio = texto.find('{')?;
    let mut prof = 0usize;
    let mut em_string = false;
    let mut escapado = false;
    for i in inicio..bytes.len() {
        let c = bytes[i];
        if em_string {
            if escapado {
                escapado = false;
            } else if c == b'\\' {
                escapado = true;
            } else if c == b'"' {
                em_string = false;
            }
            continue;
        }
        match c {
            b'"' => em_string = true,
            b'{' => prof += 1,
            b'}' => {
                prof -= 1;
                if prof == 0 {
                    return Some(&texto[inicio..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extrai + desserializa + valida. O `Err` é a mensagem que volta pro modelo na
/// segunda tentativa, então tem que ser acionável.
pub fn parse_plano(texto: &str) -> Result<Plano, String> {
    let bruto = extrai_objeto(texto).ok_or("não achei um objeto JSON na resposta")?;
    let p: Plano = serde_json::from_str(bruto).map_err(|e| format!("JSON não bate com o schema: {e}"))?;
    p.validar()?;
    Ok(p)
}

pub fn parse_veredito(texto: &str) -> Result<Veredito, String> {
    let bruto = extrai_objeto(texto).ok_or("não achei um objeto JSON na resposta")?;
    let v: Veredito = serde_json::from_str(bruto).map_err(|e| format!("JSON não bate com o schema: {e}"))?;
    v.validar()?;
    Ok(v)
}

fn uma_linha(s: &str, max: usize) -> String {
    let t: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANO_OK: &str = r#"Claro, segue o plano:
```json
{"objetivo":"corrigir o parser","passos":[
  {"id":"p1","arquivo":"src/api.rs","acao":"editar",
   "o_que":"trocar o find por uma busca com profundidade","aceite":["cargo test passa"]}
]}
```
Qualquer coisa me chame."#;

    #[test]
    fn extrai_de_dentro_da_cerca_e_da_prosa() {
        let p = parse_plano(PLANO_OK).expect("devia parsear");
        assert_eq!(p.passos.len(), 1);
        assert_eq!(p.passos[0].arquivo, "src/api.rs");
    }

    /// O motivo do contador de profundidade existir: chave dentro de string.
    /// A versão ingênua (primeiro `{` até o último `}`) engolia o `}` do texto.
    #[test]
    fn chave_dentro_de_string_nao_quebra() {
        let t = r#"{"objetivo":"x","passos":[{"id":"p1","arquivo":"a.rs","acao":"editar",
            "o_que":"substituir o bloco { ... } pelo helper novo do módulo","aceite":[]}]} e mais prosa }"#;
        let p = parse_plano(t).expect("devia parsear");
        assert!(p.passos[0].o_que.contains('{'));
    }

    #[test]
    fn recusa_plano_sem_passos_e_acao_invalida() {
        assert!(parse_plano(r#"{"objetivo":"x","passos":[]}"#).is_err());
        let t = r#"{"objetivo":"x","passos":[{"id":"p1","arquivo":"a.rs","acao":"refatorar",
            "o_que":"uma instrução suficientemente longa pra passar","aceite":[]}]}"#;
        assert!(parse_plano(t).unwrap_err().contains("acao"));
    }

    /// A validação que existe pelo modo de falha nº 1 do pipeline: instrução que
    /// só faz sentido pra quem leu o raciocínio do arquiteto.
    #[test]
    fn recusa_o_que_curto() {
        let t = r#"{"objetivo":"x","passos":[{"id":"p1","arquivo":"a.rs","acao":"editar",
            "o_que":"corrigir","aceite":[]}]}"#;
        assert!(parse_plano(t).unwrap_err().contains("sustentar sozinha"));
    }

    #[test]
    fn recusa_ids_repetidos() {
        let passo = r#"{"id":"p1","arquivo":"a.rs","acao":"editar","o_que":"uma instrução bem longa aqui","aceite":[]}"#;
        let t = format!(r#"{{"objetivo":"x","passos":[{passo},{passo}]}}"#);
        assert!(parse_plano(&t).unwrap_err().contains("repetido"));
    }

    #[test]
    fn veredito_reprovado_precisa_de_achado() {
        assert!(parse_veredito(r#"{"aprovado":false,"achados":[]}"#).is_err());
        assert!(parse_veredito(r#"{"aprovado":true}"#).unwrap().aprovado);
    }

    #[test]
    fn classe_e_chave_do_achado() {
        let t = r#"{"aprovado":false,"achados":[{"passo_id":"p1","classe":"PLANO_ERRADO",
            "aceite_violado":"cargo test passa","o_que_fazer":"o plano não cobre o caso do buffer vazio"}]}"#;
        let v = parse_veredito(t).unwrap();
        assert!(v.achados[0].plano_errado(), "classe é case-insensitive");
        assert_eq!(v.achados[0].chave(), "p1|cargo test passa");
    }
}
