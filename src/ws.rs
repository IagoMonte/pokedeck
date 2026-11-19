

use crate::{empurrar, Cfg, Estado};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

const URL_WS: &str = "wss://pokeidle.io";
/// o jogo só deixa 1 arremesso por vez; 1,1 s entre eles evita o descarte silencioso que vimos a <1 s.
const GAP_BOLA: Duration = Duration::from_millis(1100);
/// o corpo some em 30 s (TEMPO_CHAO_MS no app.js)
const VIDA_CORPO: Duration = Duration::from_secs(28);
/// o cliente oficial bloqueia nova troca por 3 s
const GAP_TROCA: Duration = Duration::from_millis(3250);

fn agora_s() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() }


#[derive(Default)]
pub struct Merge {
    campos: Map<String, Value>,
    pokes: HashMap<i64, Value>,
    pronto: bool,
}
impl Merge {
    pub fn aplicar(&mut self, parcial: &Value) {
        let Some(obj) = parcial.as_object() else { return };
        let mut resto = obj.clone();
        let cheio = resto.remove("cheio").and_then(|v| v.as_bool()).unwrap_or(false);
        let lista = resto.remove("pokemons");
        let mud = resto.remove("pkMud");
        let fora = resto.remove("pkFora");
        let dmud = resto.remove("dexMud");
        let dfora = resto.remove("dexFora");
        let por_id = |v: &Value| v["id"].as_i64().map(|i| (i, v.clone()));
        if cheio || !self.pronto {
            self.campos = resto;
            self.pokes = lista.as_ref().and_then(|l| l.as_array()).map(|a| a.iter().filter_map(por_id).collect()).unwrap_or_default();
            self.pronto = true;
            return;
        }
        for (k, v) in resto { self.campos.insert(k, v); }
        if let Some(a) = lista.as_ref().and_then(|l| l.as_array()) { self.pokes = a.iter().filter_map(por_id).collect(); }
        if let Some(a) = mud.as_ref().and_then(|l| l.as_array()) { for k in a { if let Some((i, v)) = por_id(k) { self.pokes.insert(i, v); } } }
        if let Some(a) = fora.as_ref().and_then(|l| l.as_array()) { for i in a { if let Some(i) = i.as_i64() { self.pokes.remove(&i); } } }
        if dmud.is_some() || dfora.is_some() {
            let dex = self.campos.entry("pokedex").or_insert_with(|| json!({}));
            if let (Some(d), Some(m)) = (dex.as_object_mut(), dmud.as_ref().and_then(|v| v.as_object())) { for (k, v) in m { d.insert(k.clone(), v.clone()); } }
            if let (Some(d), Some(f)) = (dex.as_object_mut(), dfora.as_ref().and_then(|v| v.as_array())) { for k in f { if let Some(k) = k.as_str() { d.remove(k); } } }
        }
    }
    pub fn montar(&self) -> Value {
        let mut o = self.campos.clone();
        o.insert("pokemons".into(), Value::Array(self.pokes.values().cloned().collect()));
        Value::Object(o)
    }
    fn i(&self, k: &str) -> Option<i64> { self.campos.get(k).and_then(|v| v.as_i64()) }
    fn s(&self, k: &str) -> Option<String> { self.campos.get(k).and_then(|v| v.as_str()).map(String::from) }
    fn ativo(&self) -> Option<i64> { self.i("activeId") }
    fn item_qtd(&self, id: i64) -> i64 { self.campos.get("items").and_then(|b| b.get(id.to_string())).and_then(|v| v.as_i64()).unwrap_or(0) }
    fn estoque(&self, bola: i64) -> i64 { self.campos.get("balls").and_then(|b| b.get(bola.to_string())).and_then(|v| v.as_i64()).unwrap_or(0) }
    fn capturada(&self, especie: i64) -> bool {
        self.campos.get("pokedex").and_then(|d| d.get(especie.to_string())).and_then(|e| e["c"].as_i64()).unwrap_or(0) == 1
    }
}


#[derive(Serialize, Clone, Default)]
pub struct Pk {
    pub id: i64,
    pub nome: String,
    pub level: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub xp: i64,
    pub xp_nivel: i64,
    pub xp_prox: i64,
    pub shiny: bool,
    pub nota: f64,
    pub ativo: bool,
}
fn pk(v: &Value, ativo: bool) -> Pk {
    Pk {
        id: v["id"].as_i64().unwrap_or(0),
        nome: v["nome"].as_str().unwrap_or("?").into(),
        level: v["level"].as_i64().unwrap_or(0),
        hp: v["hp"].as_i64().unwrap_or(0),
        max_hp: v["maxHp"].as_i64().unwrap_or(0),
        xp: v["xp"].as_i64().unwrap_or(0),
        xp_nivel: v["xpNivel"].as_i64().unwrap_or(0),
        xp_prox: v["xpProximo"].as_i64().unwrap_or(0),
        shiny: v["shiny"].as_bool().unwrap_or(false),
        nota: v["nota"].as_f64().unwrap_or(0.0),
        ativo,
    }
}

#[derive(Serialize, Clone, Default)]
pub struct Live {
    pub status: String,
    pub level: Option<i64>,
    pub xp: Option<i64>,
    pub xp_nivel: Option<i64>,
    pub xp_prox: Option<i64>,
    pub gold: Option<i64>,
    pub orbs: Option<i64>,
    pub diamonds: Option<i64>,
    pub hunt_slug: Option<String>,
    pub hunt_nome: Option<String>,
    pub no_centro: bool,
    pub vip: bool,
    pub ativo: Option<Pk>,
    pub equipe: Vec<Pk>,
    pub balls: Value,
    pub catalogo: Value,
    pub inicio: u64,
    pub kills: u64,
    pub xp_ganho: i64,
    pub ouro_ganho: i64,
    pub arremessos: u64,
    pub capturas: u64,
    pub fugas: u64,
    pub compras_bolas: u64,
    pub compras_pocoes: u64,
    pub compras_revives: u64,
    pub gasto_compras: i64,
    pub pocoes: Value,
    pub revives: Value,
    #[serde(skip)] pub heal_cat: Vec<(i64, String, i64)>,
    #[serde(skip)] pub rev_cat: Vec<(i64, String, i64)>,
    pub pulados: u64,
    pub shinys: u64,
    pub trocas: u64,
    pub fila_bolas: usize,
    pub log: Vec<String>,
}
impl Live {
    fn loga(&mut self, s: String) {
        let hora = agora_s() % 86400;
        self.log.push(format!("{:02}:{:02}:{:02} {s}", hora / 3600, hora % 3600 / 60, hora % 60));
        if self.log.len() > 14 { self.log.remove(0); }
    }
    fn atualizar(&mut self, m: &Merge, hunts: &HashMap<String, String>) {
        self.level = m.i("level");
        self.xp = m.i("xp");
        self.xp_nivel = m.i("xpNivel");
        self.xp_prox = m.i("xpProximo");
        self.gold = m.i("gold");
        self.orbs = m.i("orbs");
        self.diamonds = m.i("diamonds");
        self.hunt_slug = m.s("huntSlug");
        self.hunt_nome = self.hunt_slug.as_ref().map(|s| hunts.get(s).cloned().unwrap_or_else(|| s.clone()));
        self.no_centro = m.campos.get("noCentro").and_then(|v| v.as_bool()).unwrap_or(false);
        self.vip = m.campos.get("loja").and_then(|l| l["vip"].as_bool()).unwrap_or(false);
        self.balls = m.campos.get("balls").cloned().unwrap_or(Value::Null);
        self.pocoes = Value::Array(self.heal_cat.iter().map(|(id, nome, preco)| json!({"id": id, "nome": nome, "preco": preco, "q": m.item_qtd(*id)})).collect());
        self.revives = Value::Array(self.rev_cat.iter().map(|(id, nome, preco)| json!({"id": id, "nome": nome, "preco": preco, "q": m.item_qtd(*id)})).collect());
        let ativo = m.ativo();
        self.ativo = ativo.and_then(|id| m.pokes.get(&id)).map(|v| pk(v, true));
        let mut eq: Vec<(i64, Pk)> = m.pokes.values().filter_map(|v| v["slot"].as_i64().map(|s| (s, pk(v, Some(v["id"].as_i64().unwrap_or(-1)) == ativo)))).collect();
        eq.sort_by_key(|(s, _)| *s);
        self.equipe = eq.into_iter().map(|(_, p)| p).collect();
    }
}

enum Fim { Tomada }

fn publicar(app: &AppHandle, st: &Estado, nick: &str, live: &Live) {
    if debug() { eprintln!("[{nick}] {} | kills={} xp={} arremessos={} capturas={} fugas={} fila={} trocas={}", live.status, live.kills, live.xp_ganho, live.arremessos, live.capturas, live.fugas, live.fila_bolas, live.trocas); }
    st.lock().unwrap().live.insert(nick.to_string(), live.clone());
    empurrar(app, st);
}
fn cfg_de(st: &Estado, nick: &str) -> Cfg {
    st.lock().unwrap().contas.iter().find(|c| c.nick == nick).map(|c| c.cfg.clone()).unwrap_or_default()
}
fn debug() -> bool { std::env::var("POKEDECK_DEBUG").is_ok() }


/// Por onde falamos com o servidor: o nosso websocket (modo WS) ou o socket do próprio jogo,
/// espiado por um script injetado na janela (modo Janela). Assim captura, xpshare, autobuy e
/// troca de hunt funcionam nos dois modos com a mesma lógica.
enum Via {
    Ws(tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>),
    Ponte { app: AppHandle, nick: String, rx: tokio::sync::mpsc::UnboundedReceiver<String> },
}
impl Via {
    fn ponte(&self) -> bool { matches!(self, Via::Ponte { .. }) }
    async fn enviar(&mut self, txt: String) -> Result<(), String> {
        match self {
            Via::Ws(w) => w.send(Message::Text(txt)).await.map_err(|e| e.to_string()),
            Via::Ponte { app, nick, .. } => {
                use tauri::Manager;
                let wv = app.get_webview(&crate::label(nick)).ok_or("janela fechada")?;
                let js = format!("window.__pd_send&&window.__pd_send({})", serde_json::to_string(&txt).unwrap());
                wv.eval(&js).map_err(|e| e.to_string())
            }
        }
    }
    async fn proximo(&mut self) -> Option<Result<Message, String>> {
        match self {
            Via::Ws(w) => w.next().await.map(|r| r.map_err(|e| e.to_string())),
            Via::Ponte { rx, .. } => rx.recv().await.map(|t| Ok(Message::Text(t))),
        }
    }
}

struct Troca { id: i64, enviado: Instant }

async fn uma_conexao(app: &AppHandle, st: &Estado, nick: &str, live: &mut Live, ponte: bool) -> Result<Fim, String> {
    let mut via = if ponte {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        st.lock().unwrap().pontes.insert(nick.to_string(), tx);
        Via::Ponte { app: app.clone(), nick: nick.to_string(), rx }
    } else {
        let token = st.lock().unwrap().contas.iter().find(|c| c.nick == nick).map(|c| c.token.clone()).ok_or("conta removida")?;
        let mut req = URL_WS.into_client_request().map_err(|e| e.to_string())?;
        req.headers_mut().insert("Origin", "https://pokeidle.io".parse().unwrap());
        let (ws, _) = connect_async(req).await.map_err(|e| format!("conexão: {e}"))?;
        let mut v = Via::Ws(ws);
        let disp: String = (0..16).map(|_| format!("{:x}", rand::random::<u8>() % 16)).collect();
        v.enviar(json!({"t":"hello","nick":nick,"token":token,"delta":1,"dispositivo":disp}).to_string()).await?;
        v
    };

    let mut m = Merge::default();
    let mut hunts: HashMap<String, String> = HashMap::new();
    let mut catalogo: Vec<i64> = vec![];
    let mut spawns: HashMap<i64, (i64, String, i64)> = HashMap::new();
    let mut fila: VecDeque<(i64, Instant)> = VecDeque::new();
    let mut chances: HashMap<(i64, i64), f64> = HashMap::new();
    let mut ultimo_arremesso = Instant::now() - GAP_BOLA;

    let mut hits: VecDeque<f64> = VecDeque::new();
    let mut limiar: f64 = cfg_de(st, nick).xps.limiar as f64;
    let mut sw = false;
    let mut troca: Option<Troca> = None;

    let mut precos_bola: HashMap<i64, i64> = HashMap::new();
    let mut nomes_bola: Vec<String> = vec![];
    let mut ult_compra = Instant::now();
    let mut gastos: VecDeque<(Instant, i64)> = VecDeque::new();
    let mut hunt_ctx = Value::Null;
    let mut proximo_calc: Option<Instant> = None;
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut sujo = false;
    let mut ultimo_pub = Instant::now();

    loop {
        tokio::select! {
            _ = ping.tick() => { if !via.ponte() { via.enviar(r#"{"t":"ping"}"#.into()).await?; } }
            _ = tick.tick() => {
                let cfg = cfg_de(st, nick);
                let agora = Instant::now();

                while fila.front().map_or(false, |(_, ex)| *ex <= agora) { fila.pop_front(); }
                if cfg.captura.ligado && agora.duration_since(ultimo_arremesso) >= GAP_BOLA {
                    if let Some((slot, _)) = fila.pop_front() {
                        let cat = catalogo.clone();
                        let bola = if cfg.captura.bola > 0 && m.estoque(cfg.captura.bola) > 0 { Some(cfg.captura.bola) } else { cat.into_iter().find(|&b| m.estoque(b) > 0) };
                        match bola {
                            Some(b) => {
                                via.enviar(json!({"t":"ball.throw","ballId":b,"slot":slot}).to_string()).await?;
                                ultimo_arremesso = agora;
                                live.arremessos += 1;
                                sujo = true;
                            }
                            None => { fila.clear(); live.loga("sem bolas — captura parada".into()); sujo = true; }
                        }
                    }
                }

                if cfg.buy.ligado && m.pronto && agora.duration_since(ult_compra) >= Duration::from_secs(3) {
                    ult_compra = agora;
                    while gastos.front().map_or(false, |(t, _)| agora.duration_since(*t) > Duration::from_secs(3600)) { gastos.pop_front(); }
                    let gasto_h: i64 = gastos.iter().map(|(_, g)| *g).sum();
                    let mut ouro = m.i("gold").unwrap_or(0);
                    let mut orc = (cfg.buy.limite_hora - gasto_h).max(0);
                    let b = &cfg.buy;
                    let mut pedidos: Vec<(&str, i64, i64)> = vec![];
                    let mut planejar = |kind: &'static str, id: i64, preco: i64, tem: i64, min: i64, alvo: i64, pedidos: &mut Vec<(&str, i64, i64)>| {
                        if preco <= 0 || tem >= min { return; }
                        let q = (alvo - tem).min(500).min(((ouro - b.reserva) / preco).max(0)).min(orc / preco);
                        if q > 0 { pedidos.push((kind, id, q)); ouro -= q * preco; orc -= q * preco; }
                    };
                    if b.bola_on {
                        let id = if b.bola > 0 { b.bola } else { m.campos.get("automation").and_then(|a| a["ballIds"].get(0)).and_then(|v| v.as_i64()).unwrap_or(1) };
                        planejar("ball", id, precos_bola.get(&id).copied().unwrap_or(0), m.estoque(id), b.bola_min, b.bola_alvo, &mut pedidos);
                    }
                    if b.poc_on {
                        let auto_ids: Vec<i64> = m.campos.get("automation").and_then(|a| a["potionIds"].as_array()).map(|a| a.iter().filter_map(|v| v.as_i64()).collect()).unwrap_or_default();
                        let id = if b.poc > 0 { Some(b.poc) } else { auto_ids.iter().copied().find(|i| live.heal_cat.iter().any(|(h, _, p)| h == i && *p <= 10_000)) };
                        if let Some(id) = id {
                            let preco = live.heal_cat.iter().find(|(h, _, _)| *h == id).map(|(_, _, p)| *p).unwrap_or(0);
                            planejar("item", id, preco, m.item_qtd(id), b.poc_min, b.poc_alvo, &mut pedidos);
                        }
                    }
                    if b.rev_on {
                        let auto_ids: Vec<i64> = m.campos.get("automation").and_then(|a| a["reviveIds"].as_array()).map(|a| a.iter().filter_map(|v| v.as_i64()).collect()).unwrap_or_default();
                        let id = if b.rev > 0 { Some(b.rev) } else { auto_ids.iter().copied().find(|i| live.rev_cat.iter().any(|(h, _, p)| h == i && *p <= 10_000)) };
                        if let Some(id) = id {
                            let preco = live.rev_cat.iter().find(|(h, _, _)| *h == id).map(|(_, _, p)| *p).unwrap_or(0);
                            planejar("item", id, preco, m.item_qtd(id), b.rev_min, b.rev_alvo, &mut pedidos);
                        }
                    }
                    for (kind, id, q) in pedidos {
                        via.enviar(json!({"t":"shop.buy","kind":kind,"id":id,"qty":q}).to_string()).await?;
                    }
                }

                if let Some(t) = &mut troca {
                    if m.ativo() == Some(t.id) { troca = None; }
                    else if agora.duration_since(t.enviado) >= GAP_TROCA {
                        via.enviar(json!({"t":"team.active","pokemonId":t.id}).to_string()).await?;
                        t.enviado = agora;
                    }
                }

                let msgs = st.lock().unwrap().saida.remove(nick).unwrap_or_default();
                for msg in msgs {
                    if msg["t"] == "hunt.select" {
                        via.enviar(msg.to_string()).await?;
                        live.loga(format!("hunt → {}", msg["slug"].as_str().unwrap_or("?")));
                        sujo = true;
                        proximo_calc = Some(Instant::now() + Duration::from_secs(6));
                    }
                }

                let pedido = st.lock().unwrap().hunt_pedido.remove(nick);
                if !hunt_ctx.is_null() && m.ativo().is_some() && (pedido || proximo_calc.map_or(false, |t| agora >= t)) {
                    proximo_calc = Some(agora + Duration::from_secs(15 * 60));
                    let entrada = json!({"welcome": hunt_ctx, "eu": m.montar()}).to_string();
                    tauri::async_runtime::spawn(calcular_hunt(app.clone(), st.clone(), nick.to_string(), entrada));
                }
                live.fila_bolas = fila.len();
                if sujo && ultimo_pub.elapsed() > Duration::from_millis(600) {
                    live.atualizar(&m, &hunts);
                    publicar(app, st, nick, live);
                    sujo = false; ultimo_pub = Instant::now();
                }
            }
            msg = via.proximo() => {
                let msg = match msg { Some(Ok(x)) => x, Some(Err(e)) => return Err(e), None => return Err("fechado".into()) };
                match msg {
                    Message::Close(cf) => {
                        if cf.as_ref().map_or(false, |c| u16::from(c.code) == 4001) { return Ok(Fim::Tomada); }
                        return Err(format!("fechado pelo servidor ({:?})", cf.map(|c| u16::from(c.code))));
                    }
                    Message::Text(txt) => {
                        let v: Value = match serde_json::from_str(&txt) { Ok(v) => v, Err(_) => continue };
                        let t = v["t"].as_str().unwrap_or("");
                        if t == "__fechou" { live.status = "janela reconectando…".into(); sujo = true; continue; }
                        if t.contains("sessaoTomada") || t.contains("sessao.tomada") { return Ok(Fim::Tomada); }
                        match t {
                            "welcome" => {
                                if !via.ponte() { via.enviar(json!({"t":"cliente.economia","ativo":true}).to_string()).await?; }
                                m = Merge::default();
                                m.aplicar(&v["estado"]);
                                hunts = v["hunts"].as_array().map(|a| a.iter().filter_map(|h| Some((h["slug"].as_str()?.to_string(), h["nome"].as_str()?.to_string()))).collect()).unwrap_or_default();
                                let cat = v["catalogoBolas"].as_array().cloned().unwrap_or_default();
                                catalogo = cat.iter().filter_map(|b| b["id"].as_i64()).collect();
                                precos_bola = cat.iter().filter_map(|b| Some((b["id"].as_i64()?, b["priceGold"].as_i64()?))).collect();
                                nomes_bola = cat.iter().filter_map(|b| b["nome"].as_str().map(String::from)).collect();
                                let todos = tauri::async_runtime::spawn_blocking(itens_de_cura).await.unwrap_or_default();
                                live.heal_cat = todos.iter().filter(|x| !x.3).map(|x| (x.0, x.1.clone(), x.2)).collect();
                                live.rev_cat = todos.iter().filter(|x| x.3).map(|x| (x.0, x.1.clone(), x.2)).collect();
                                if debug() { eprintln!("[{nick}] poções compráveis: {}; revives: {}; bolas c/ preço: {}", live.heal_cat.len(), live.rev_cat.len(), precos_bola.len()); }
                                live.catalogo = Value::Array(cat.iter().map(|b| json!({"id":b["id"],"nome":b["nome"],"preco":b["priceGold"],"compravel":b["compravel"]})).collect());
                                if live.inicio == 0 { live.inicio = agora_s(); }
                                hunt_ctx = json!({
                                    "hunts": v["hunts"], "tabelaTipos": v["tabelaTipos"], "ampliacaoHunt": v["ampliacaoHunt"],
                                    "multHpSelvagem": v["multHpSelvagem"], "multDanoSelvagem": v["multDanoSelvagem"], "tiposBoost": v["tiposBoost"],
                                    "bicicletas": v["bicicletas"], "chanceBossToken": v["chanceBossToken"], "outlandTiers": v["outlandTiers"],
                                    "casas": v["casas"], "shinyStone": v["shinyStone"], "catalogoBolas": v["catalogoBolas"], "itensCompraveis": v["itensCompraveis"],
                                });
                                proximo_calc = Some(Instant::now() + Duration::from_secs(5));
                                {
                                    let cat: Vec<Value> = v["hunts"].as_array().map(|a| a.iter().map(|h| json!([h["slug"], h["nome"], h["nivel"], h["area"]])).collect()).unwrap_or_default();
                                    let mut rt = st.lock().unwrap();
                                    rt.hunt_cat.insert(nick.to_string(), Value::Array(cat.clone()));
                                    drop(rt);
                                    use tauri::Emitter;
                                    let _ = app.emit("hunt_info", json!({"nick": nick, "catalogo": cat}));
                                }
                                live.status = if via.ponte() { "jogando (janela)".into() } else { "online (ws)".into() };
                                live.loga("conectado".into());
                                live.atualizar(&m, &hunts);
                                publicar(app, st, nick, live);
                            }
                            "estado" => { m.aplicar(&v["estado"]); sujo = true; }
                            "sessao.renovada" => {
                                if let Some(tk) = v["token"].as_str() {
                                    let mut rt = st.lock().unwrap();
                                    if let Some(c) = rt.contas.iter_mut().find(|c| c.nick == nick) { c.token = tk.to_string(); }
                                    crate::salvar(app, &rt);
                                }
                            }
                            "batalha" => {
                                let cfg = cfg_de(st, nick);
                                for e in v["ev"].as_array().cloned().unwrap_or_default() {
                                    let k = e["k"].as_str().unwrap_or("");
                                    match k {
                                        "spawn" => {
                                            if spawns.len() > 400 { spawns.clear(); }
                                            if let Some(s) = e["slot"].as_i64() { spawns.insert(s, (e["speciesId"].as_i64().unwrap_or(0), e["nome"].as_str().unwrap_or("?").into(), e["level"].as_i64().unwrap_or(0))); }
                                        }
                                        "ataque" if e["por"] == "jogador" => {
                                            if cfg.xps.ligado {
                                                let ativo = m.ativo();
                                                let dano = e["dano"].as_f64().unwrap_or(0.0);
                                                if cfg.xps.auto && ativo == Some(cfg.xps.finalizador) {
                                                    hits.push_back(dano); if hits.len() > 25 { hits.pop_front(); }
                                                    if hits.len() >= 8 { let mut h: Vec<f64> = hits.iter().copied().collect(); h.sort_by(|a, b| a.partial_cmp(b).unwrap()); limiar = (h[(h.len() as f64 * 0.75) as usize] * 1.1).round().max(100.0); }
                                                }
                                                let th = if cfg.xps.auto { limiar } else { cfg.xps.limiar as f64 };
                                                let hp = e["hpAlvo"].as_f64().unwrap_or(0.0);
                                                if hp > 0.0 && hp <= th && ativo == Some(cfg.xps.atacante) && !sw {
                                                    sw = true;
                                                    troca = Some(Troca { id: cfg.xps.finalizador, enviado: Instant::now() });
                                                    via.enviar(json!({"t":"team.active","pokemonId":cfg.xps.finalizador}).to_string()).await?;
                                                    live.trocas += 1;
                                                    live.loga(format!("xpshare → finalizador (alvo {hp:.0} hp ≤ {th:.0})"));
                                                    sujo = true;
                                                }
                                            }
                                        }
                                        "morte" if e["quem"] == "selvagem" => {
                                            live.kills += 1;
                                            live.xp_ganho += e["xp"].as_i64().unwrap_or(0);
                                            live.ouro_ganho += e["ouro"].as_i64().unwrap_or(0);
                                            sujo = true;
                                            if cfg.xps.ligado {
                                                let ativo = m.ativo();
                                                if cfg.xps.auto && ativo != Some(cfg.xps.finalizador) { limiar = (limiar * 1.2).round(); }
                                                if ativo == Some(cfg.xps.finalizador) {
                                                    troca = Some(Troca { id: cfg.xps.atacante, enviado: Instant::now() });
                                                    via.enviar(json!({"t":"team.active","pokemonId":cfg.xps.atacante}).to_string()).await?;
                                                }
                                                sw = false;
                                            }
                                            if cfg.captura.ligado {
                                                if let Some(slot) = e["slot"].as_i64() {
                                                    let esp = spawns.get(&slot).map(|(e, _, _)| *e);
                                                    let mut ok = match cfg.captura.modo.as_str() {
                                                        "novos" => esp.map_or(false, |e| !m.capturada(e)),
                                                        _ => true,
                                                    };

                                                    let bola = if cfg.captura.bola > 0 { cfg.captura.bola } else { catalogo.iter().copied().find(|&b| m.estoque(b) > 0).unwrap_or(0) };
                                                    if let Some(c) = esp.and_then(|e| chances.get(&(e, bola))) { if *c * 100.0 < cfg.captura.chance_min { ok = false; live.pulados += 1; } }
                                                    if ok { fila.push_back((slot, Instant::now() + VIDA_CORPO)); }
                                                }
                                            }
                                        }
                                        "bola" => {
                                            if let (Some(sl), Some(ch), Some(b)) = (e["slot"].as_i64(), e["chance"].as_f64(), e["ballId"].as_i64()) {
                                                if let Some((esp, _, _)) = spawns.get(&sl) { chances.insert((*esp, b), ch); }
                                            }
                                        }
                                        "compra" => {
                                            let (gasto, qtd, nome) = (e["gasto"].as_i64().unwrap_or(0), e["qtd"].as_u64().unwrap_or(0), e["nome"].as_str().unwrap_or("?"));
                                            gastos.push_back((Instant::now(), gasto));
                                            live.gasto_compras += gasto;
                                            if nomes_bola.iter().any(|n| n == nome) { live.compras_bolas += qtd; } else if live.rev_cat.iter().any(|(_, n, _)| n == nome) { live.compras_revives += qtd; } else { live.compras_pocoes += qtd; }
                                            live.loga(format!("comprou {qtd}× {nome} ({gasto} coins)"));
                                            if debug() { eprintln!("[{nick}] compra {qtd}x {nome} gasto={gasto}"); }
                                            sujo = true;
                                        }
                                        "fugiu" => { live.fugas += 1; sujo = true; }
                                        "capturado" => {
                                            live.capturas += 1;
                                            let p = &e["pokemon"];
                                            let sh = p["shiny"].as_bool().unwrap_or(false);
                                            if sh { live.shinys += 1; }
                                            live.loga(format!("capturou {}{} Nv{}", if sh { "✨ " } else { "" }, p["nome"].as_str().unwrap_or("?"), p["level"]));
                                            sujo = true;
                                        }
                                        "hunt" => { live.loga(format!("hunt: {}", e["nome"].as_str().unwrap_or("?"))); sujo = true; }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub async fn laco(app: AppHandle, st: Estado, nick: String, ponte: bool) {
    let mut espera = 5u64;
    let mut live = st.lock().unwrap().live.get(&nick).cloned().unwrap_or_default();
    loop {
        live.status = "conectando…".into();
        publicar(&app, &st, &nick, &live);
        let r = uma_conexao(&app, &st, &nick, &mut live, ponte).await;
        if debug() { eprintln!("[{nick}] conexão saiu: {:?}", r.as_ref().err()); }
        match r {
            Ok(Fim::Tomada) => {
                live.status = "sessão tomada por outro cliente".into();
                live.loga("sessão tomada — parei".into());
                publicar(&app, &st, &nick, &live);
                return;
            }
            Err(e) if ponte => {
                live.status = format!("janela: ponte encerrada ({e})");
                publicar(&app, &st, &nick, &live);
                return;
            }
            Err(e) => {
                live.status = format!("caiu ({e}); retry {espera}s");
                publicar(&app, &st, &nick, &live);
                tokio::time::sleep(Duration::from_secs(espera)).await;
                espera = (espera * 2).min(120);
            }
        }
    }
}

/// Roda o motor do /hunt (hunt/calcular.mjs, em Node) com o estado atual da conta.
async fn calcular_hunt(app: AppHandle, st: Estado, nick: String, entrada: String) {
    use tauri::Emitter;
    let dir = pasta_hunt();
    let define = |st: &Estado, txt: String, rank: Option<Value>| {
        let mut rt = st.lock().unwrap();
        rt.hunt_status.insert(nick.clone(), txt.clone());
        if let Some(r) = &rank { rt.hunt_rank.insert(nick.clone(), r.clone()); }
        drop(rt);
        if debug() { eprintln!("[{nick}] hunt: {txt}"); }
        let _ = app.emit("hunt_info", json!({"nick": nick, "rank": rank, "status": txt}));
    };
    define(&st, "calculando…".into(), None);
    let res = tauri::async_runtime::spawn_blocking(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut cmd = Command::new("node");
        cmd.arg(format!("{dir}/calcular.mjs")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000); }
        let mut c = cmd.spawn().map_err(|e| format!("não consegui rodar o Node ({e}) — instale o Node.js (nodejs.org)"))?;
        c.stdin.take().unwrap().write_all(entrada.as_bytes()).map_err(|e| e.to_string())?;
        let o = c.wait_with_output().map_err(|e| e.to_string())?;
        if !o.status.success() { return Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("erro").to_string()); }
        serde_json::from_slice::<Value>(&o.stdout).map_err(|e| format!("saída inválida: {e}"))
    }).await;
    let hora = agora_s() % 86400;
    match res {
        Ok(Ok(r)) => define(&st, format!("ok · {:02}:{:02}", hora / 3600, hora % 3600 / 60), Some(r)),
        Ok(Err(e)) => define(&st, format!("erro: {e}"), None),
        Err(e) => define(&st, format!("erro: {e}"), None),
    }
}

/// HTTP(S) cliente (ureq precisa do conector TLS explícito com a feature native-tls).
pub fn agente() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("TLS");
    ureq::AgentBuilder::new().tls_connector(std::sync::Arc::new(tls)).timeout(Duration::from_secs(20)).build()
}
static HEAL: std::sync::Mutex<Vec<(i64, String, i64, bool)>> = std::sync::Mutex::new(Vec::new());
/// Poções compráveis no NPC (category=heal, npcPrice>0), do assets/items.json do jogo; em cache entre reconexões.
fn itens_de_cura() -> Vec<(i64, String, i64, bool)> {
    if let Some(c) = Some(HEAL.lock().unwrap().clone()).filter(|c| !c.is_empty()) { return c; }
    let v: Option<Value> = agente().get("https://pokeidle.io/assets/items.json").call().ok().and_then(|r| r.into_json().ok());
    let mut l: Vec<(i64, String, i64, bool)> = v.as_ref().and_then(|v| v["items"].as_array().or(v.as_array())).map(|a| a.iter()
        .filter(|i| (i["category"] == "heal" || i["category"] == "revive") && i["npcPrice"].as_i64().unwrap_or(0) > 0)
        .filter_map(|i| Some((i["id"].as_i64()?, i["name"].as_str()?.to_string(), i["npcPrice"].as_i64()?, i["category"] == "revive"))).collect()).unwrap_or_default();
    l.sort_by_key(|(_, _, p, _)| *p);
    if !l.is_empty() { *HEAL.lock().unwrap() = l.clone(); }
    l
}

/// Onde está `hunt/calcular.mjs`: POKEDECK_HUNT_DIR, ao lado do executável (pacote), ou a pasta do código (dev).
fn pasta_hunt() -> String {
    if let Ok(d) = std::env::var("POKEDECK_HUNT_DIR") { return d; }
    if let Some(exe) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        for c in [exe.join("hunt"), exe.join("resources").join("hunt")] { if c.join("calcular.mjs").exists() { return c.to_string_lossy().into_owned(); } }
    }
    concat!(env!("CARGO_MANIFEST_DIR"), "/hunt").to_string()
}
