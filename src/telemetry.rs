//! Tokens/custo por turno (do evento `usage`; padrão 2.15.9: `US$` exato quando
//! o provedor informa, `≈US$` estimado quando cai no tokens×pricing).

use crate::types::Usage;

#[derive(Default)]
pub struct Telemetry {
    pub tokens: i64,
    pub cost: f64,
    pub estimated: bool,
}

impl Telemetry {
    pub fn add(&mut self, u: &Usage) {
        self.tokens += u.total_tokens;
        if let Some(c) = u.cost {
            self.cost += c;
        } else if let Some(c) = u.cost_estimated {
            self.cost += c;
            self.estimated = true;
        }
    }

    pub fn footer(&self) -> String {
        let sign = if self.estimated { "≈" } else { "" };
        format!("⏱ {} tokens · custo {sign}US$ {:.6}", self.tokens, self.cost)
    }
}
