//! Export HTML do `anna stats` (`anna stats --html`). Gera uma página **única e
//! offline** (CSS/JS embutidos, zero rede) fiel ao print: tema escuro, os 8
//! tiles, o heatmap azul, abas (Visão Geral/Modelos) e filtros (Todos/30d/7d)
//! FUNCIONANDO no navegador. Sem duplicar regra de negócio: o Rust pré-computa
//! as três faixas com o `stats::compute` e serializa views prontas (strings já
//! formatadas + níveis do heatmap); o JS só decide qual view desenhar.

use crate::session::Session;
use crate::stats::{self, Range, Stats};
use serde_json::json;

/// Monta o HTML completo a partir das sessões.
pub fn build(sessions: &[Session], name: &str) -> String {
    let data = json!({
        "all": view(&stats::compute(sessions, Range::All)),
        "d30": view(&stats::compute(sessions, Range::D30)),
        "d7":  view(&stats::compute(sessions, Range::D7)),
    });
    let data_json = data.to_string().replace("</", "<\\/"); // seguro dentro de <script>

    TEMPLATE
        .replace("__NAME__", &esc(name))
        .replace("__DATA__", &data_json)
}

/// Uma view pronta para render (tudo já formatado no Rust).
fn view(s: &Stats) -> serde_json::Value {
    let tokens = if s.tokens_known {
        stats::fmt_compact(s.tokens)
    } else {
        "—".into()
    };
    let peak = s
        .peak_hour
        .map(stats::fmt_hour)
        .unwrap_or_else(|| "—".into());
    let model = s.favorite_model.clone().unwrap_or_else(|| "—".into());

    // Heatmap: colunas de 7 células; cada célula = null (padding) ou [nível, contagem].
    let weeks: Vec<serde_json::Value> = s
        .heatmap
        .weeks
        .iter()
        .map(|col| {
            let days: Vec<serde_json::Value> = col
                .iter()
                .map(|cell| match cell {
                    None => serde_json::Value::Null,
                    Some(c) => json!([stats::level(*c, s.heatmap.max), c]),
                })
                .collect();
            serde_json::Value::Array(days)
        })
        .collect();

    let models: Vec<serde_json::Value> = s
        .models
        .iter()
        .map(|r| {
            json!({
                "model": r.model,
                "sessions": stats::fmt_int(r.sessions as i64),
                "messages": stats::fmt_int(r.messages as i64),
                "tokens": if r.tokens > 0 { stats::fmt_compact(r.tokens) } else { "—".into() },
                "cost": if r.cost > 0.0 { format!("US$ {:.2}", r.cost) } else { "—".to_string() },
            })
        })
        .collect();

    json!({
        "sessions": stats::fmt_int(s.sessions as i64),
        "messages": stats::fmt_int(s.messages as i64),
        "tokens": tokens,
        "activeDays": stats::fmt_int(s.active_days as i64),
        "curStreak": format!("{}d", s.current_streak),
        "longStreak": format!("{}d", s.longest_streak),
        "peak": peak,
        "model": model,
        "weeks": weeks,
        "models": models,
        "moby": stats::moby_phrase(s.tokens),
        "cost": if s.cost > 0.0 { format!("Custo total ≈ US$ {:.2}", s.cost) } else { String::new() },
        "empty": s.sessions == 0,
        "modelKnown": s.model_known,
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const TEMPLATE: &str = r#"<!doctype html>
<html lang="pt-BR"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ShvIA Code — O que vem a seguir</title>
<style>
  :root{
    --bg:#0a0d14; --panel:#10151f; --border:#232b38; --ink:#e6edf3; --dim:#8b96a5;
    --accent:#4c9aff; --spark:#ff7a45;
    --l0:#1b2231; --l1:#16324f; --l2:#1e5aa8; --l3:#2f81f7; --l4:#5aa2ff;
  }
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--ink);
    font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;
    display:flex;justify-content:center;padding:32px 16px}
  .app{width:100%;max-width:620px}
  h1{font-size:20px;font-weight:600;margin:0 0 18px;display:flex;align-items:center;gap:8px}
  h1 .s{color:var(--spark)}
  .panel{background:var(--panel);border:1px solid var(--border);border-radius:12px;padding:16px}
  .bar{display:flex;justify-content:space-between;align-items:center;margin-bottom:14px}
  .tabs{display:flex;gap:16px}
  .tab{background:none;border:0;color:var(--dim);font:inherit;font-weight:500;cursor:pointer;padding:2px 0}
  .tab.on{color:var(--ink);border-bottom:2px solid var(--accent);padding-bottom:2px}
  .pills{display:flex;background:#0c1119;border:1px solid var(--border);border-radius:8px;overflow:hidden}
  .pill{background:none;border:0;color:var(--dim);font:inherit;font-size:12px;cursor:pointer;padding:4px 10px}
  .pill.on{background:var(--accent);color:#04101f;font-weight:600}
  .grid{display:grid;grid-template-columns:repeat(4,1fr);gap:1px;background:var(--border);
    border:1px solid var(--border);border-radius:8px;overflow:hidden}
  .cell{background:var(--panel);padding:10px 12px}
  .cell .k{color:var(--dim);font-size:11px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .cell .v{font-size:18px;font-weight:600;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .heat{display:grid;grid-auto-flow:column;grid-template-rows:repeat(7,11px);grid-auto-columns:11px;justify-content:start;gap:3px;margin:16px 2px 4px}
  .d{width:11px;height:11px;border-radius:2px;background:var(--l0)}
  .d.pad{background:transparent}
  .d.l0{background:var(--l0)} .d.l1{background:var(--l1)} .d.l2{background:var(--l2)}
  .d.l3{background:var(--l3)} .d.l4{background:var(--l4)}
  .legend{display:flex;align-items:center;gap:4px;color:var(--dim);font-size:11px;margin-top:8px}
  .legend .d{margin:0}
  .foot{color:var(--dim);font-size:13px;margin-top:16px}
  table{width:100%;border-collapse:collapse;margin-top:4px}
  th,td{text-align:right;padding:7px 8px;border-bottom:1px solid var(--border)}
  th:first-child,td:first-child{text-align:left}
  th{color:var(--dim);font-weight:500;font-size:12px}
  td.m{color:var(--accent);font-weight:500}
  .empty{color:var(--dim);padding:24px 8px;text-align:center}
  @media (max-width:520px){.grid{grid-template-columns:repeat(2,1fr)}}
</style>
</head><body>
<div class="app">
  <h1><span class="s">✳</span> O que vem a seguir, __NAME__?</h1>
  <div class="panel">
    <div class="bar">
      <div class="tabs">
        <button class="tab on" data-tab="overview">Visão Geral</button>
        <button class="tab" data-tab="models">Modelos</button>
      </div>
      <div class="pills">
        <button class="pill on" data-range="all">Todos</button>
        <button class="pill" data-range="d30">30d</button>
        <button class="pill" data-range="d7">7d</button>
      </div>
    </div>
    <div id="body"></div>
  </div>
</div>
<script>
const DATA = __DATA__;
let range = "all", tab = "overview";

function tile(k, v){ return `<div class="cell"><div class="k">${k}</div><div class="v">${v}</div></div>`; }

function overview(v){
  if(v.empty) return `<div class="empty">Sem sessões neste período — converse com a Anna e volte aqui.</div>`;
  const g = [
    tile("Sessões", v.sessions), tile("Mensagens", v.messages),
    tile("Total de tokens", v.tokens), tile("Dias ativos", v.activeDays),
    tile("Seq. atual", v.curStreak), tile("Maior seq.", v.longStreak),
    tile("Horário de pico", v.peak), tile("Modelo favorito ★", v.model),
  ].join("");
  let cells = "";
  for(const col of v.weeks){
    for(const day of col){
      if(day===null) cells += `<div class="d pad"></div>`;
      else cells += `<div class="d l${day[0]}" title="${day[1]} sessão(ões)"></div>`;
    }
  }
  const legend = `<div class="legend">menos ${[0,1,2,3,4].map(l=>`<span class="d l${l}"></span>`).join("")} mais</div>`;
  const bits = [v.moby, v.cost].filter(Boolean).join(" · ");
  const foot = bits ? `<div class="foot">${bits}</div>` : "";
  return `<div class="grid">${g}</div><div class="heat">${cells}</div>${legend}${foot}`;
}

function models(v){
  if(!v.models.length) return `<div class="empty">Sem dados de modelo neste período.</div>`;
  const rows = v.models.map(r=>`<tr><td class="m">${r.model}</td><td>${r.sessions}</td><td>${r.messages}</td><td>${r.tokens}</td><td>${r.cost}</td></tr>`).join("");
  const note = v.modelKnown ? "" : `<div class="foot">O modelo passa a ser registrado a partir das próximas sessões.</div>`;
  return `<table><thead><tr><th>Modelo</th><th>Sessões</th><th>Mensagens</th><th>Tokens</th><th>Custo</th></tr></thead><tbody>${rows}</tbody></table>${note}`;
}

function draw(){
  const v = DATA[range];
  document.getElementById("body").innerHTML = tab==="overview" ? overview(v) : models(v);
  document.querySelectorAll(".tab").forEach(b=>b.classList.toggle("on", b.dataset.tab===tab));
  document.querySelectorAll(".pill").forEach(b=>b.classList.toggle("on", b.dataset.range===range));
}
document.querySelectorAll(".tab").forEach(b=>b.onclick=()=>{tab=b.dataset.tab;draw();});
document.querySelectorAll(".pill").forEach(b=>b.onclick=()=>{range=b.dataset.range;draw();});
draw();
</script>
</body></html>
"#;
