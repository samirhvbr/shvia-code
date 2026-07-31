//! Papéis do pipeline (arquiteto → executor → revisor) e o **elenco**: a tabela
//! papel → (perfil, esforço) que mora no `config.toml`.
//!
//! A tabela é ESTÁTICA de propósito. Um modelo-roteador escolhendo o perfil por
//! pedido seria mais uma chamada paga, erraria a classificação e não daria pra
//! auditar; a tabela o dev lê no `anna config` e versiona junto com o resto.
//!
//! `so_leitura()` não é conselho de prompt: o `runner::SomenteLeitura` filtra as
//! ferramentas oferecidas E recusa a chamada. Arquiteto e revisor não escrevem
//! porque não têm como, não porque foram instruídos a não escrever.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Papel {
    Arquiteto,
    Executor,
    Revisor,
}

impl Papel {
    pub fn chave(self) -> &'static str {
        match self {
            Papel::Arquiteto => "arquiteto",
            Papel::Executor => "executor",
            Papel::Revisor => "revisor",
        }
    }

    /// Papéis que só inspecionam. O executor é o único que muda o disco.
    pub fn so_leitura(self) -> bool {
        !matches!(self, Papel::Executor)
    }

    /// Esforço quando nem o elenco nem o `effort` global dizem nada: o arquiteto
    /// pensa (`xhigh` — é a fase que define o resto), executor e revisor
    /// trabalham (`high`). Um `effort` explícito do dev sempre ganha disto.
    pub fn esforco_padrao(self) -> &'static str {
        match self {
            Papel::Arquiteto => "xhigh",
            Papel::Executor | Papel::Revisor => "high",
        }
    }
}

/// Perfil/esforço de um papel no `config.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PapelCfg {
    pub profile: Option<String>,
    pub effort: Option<String>,
}

/// ```toml
/// [elenco.arquiteto]
/// profile = "opus5@shvia"
/// effort  = "xhigh"
///
/// [elenco.executor]
/// profile = "sonnet5@shvia"
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Elenco {
    pub arquiteto: Option<PapelCfg>,
    pub executor: Option<PapelCfg>,
    pub revisor: Option<PapelCfg>,
}

impl Elenco {
    pub fn get(&self, p: Papel) -> Option<&PapelCfg> {
        match p {
            Papel::Arquiteto => self.arquiteto.as_ref(),
            Papel::Executor => self.executor.as_ref(),
            Papel::Revisor => self.revisor.as_ref(),
        }
    }

    /// `true` se nenhum papel foi configurado — o pipeline roda mesmo assim (cai
    /// no perfil global), mas vale avisar que é um blend de um modelo só.
    pub fn vazio(&self) -> bool {
        self.arquiteto.is_none() && self.executor.is_none() && self.revisor.is_none()
    }
}
