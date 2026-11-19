

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ws;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    async_runtime::{spawn, JoinHandle},
    webview::WebviewBuilder,
    window::WindowBuilder,
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WindowEvent,
};

const URL_JOGO: &str = "https://pokeidle.io/app";
const PAINEL_TRADE: &str = "http://127.0.0.1:8765";
const TRADE_VIEW: &str = "◆ Trade";
const SIDEBAR: f64 = 330.0;
const BARRA: f64 = 46.0;


fn t() -> bool { true }
fn modo_tudo() -> String { "tudo".into() }
fn limiar0() -> i64 { 1100 }

#[derive(Serialize, Deserialize, Clone)]
pub struct CapCfg {
    #[serde(default)] pub ligado: bool,
    #[serde(default = "modo_tudo")] pub modo: String,
    #[serde(default)] pub bola: i64,
    #[serde(default = "chance0")] pub chance_min: f64,
}
fn chance0() -> f64 { 1.0 }
impl Default for CapCfg { fn default() -> Self { Self { ligado: false, modo: modo_tudo(), bola: 0, chance_min: chance0() } } }

#[derive(Serialize, Deserialize, Clone)]
pub struct XpsCfg {
    #[serde(default)] pub ligado: bool,
    #[serde(default)] pub atacante: i64,
    #[serde(default)] pub finalizador: i64,
    #[serde(default = "t")] pub auto: bool,
    #[serde(default = "limiar0")] pub limiar: i64,
}
impl Default for XpsCfg { fn default() -> Self { Self { ligado: false, atacante: 0, finalizador: 0, auto: true, limiar: limiar0() } } }

fn bmin() -> i64 { 100 }
fn balvo() -> i64 { 300 }
fn pmin() -> i64 { 20 }
fn palvo() -> i64 { 100 }
fn rmin() -> i64 { 5 }
fn ralvo() -> i64 { 20 }
fn reserva0() -> i64 { 5_000_000 }
fn limite0() -> i64 { 5_000_000 }

/// Autobuy: repõe bolas e poções no NPC quando o estoque cai abaixo do mínimo, até o alvo.
#[derive(Serialize, Deserialize, Clone)]
pub struct BuyCfg {
    #[serde(default)] pub ligado: bool,
    #[serde(default = "t")] pub bola_on: bool,
    #[serde(default)] pub bola: i64,
    #[serde(default = "bmin")] pub bola_min: i64,
    #[serde(default = "balvo")] pub bola_alvo: i64,
    #[serde(default = "t")] pub poc_on: bool,
    #[serde(default)] pub poc: i64,
    #[serde(default = "pmin")] pub poc_min: i64,
    #[serde(default = "palvo")] pub poc_alvo: i64,
    #[serde(default = "t")] pub rev_on: bool,
    #[serde(default)] pub rev: i64,
    #[serde(default = "rmin")] pub rev_min: i64,
    #[serde(default = "ralvo")] pub rev_alvo: i64,
    #[serde(default = "reserva0")] pub reserva: i64,
    #[serde(default = "limite0")] pub limite_hora: i64,
}
impl Default for BuyCfg { fn default() -> Self { Self { ligado: false, bola_on: true, bola: 0, bola_min: bmin(), bola_alvo: balvo(), poc_on: true, poc: 0, poc_min: pmin(), poc_alvo: palvo(), rev_on: true, rev: 0, rev_min: rmin(), rev_alvo: ralvo(), reserva: reserva0(), limite_hora: limite0() } } }

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Cfg {
    #[serde(default)] pub buy: BuyCfg,
    #[serde(default)] pub captura: CapCfg,
    #[serde(default)] pub xps: XpsCfg,
    #[serde(default)] pub role: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Guardada {
    pub nick: String,
    pub token: String,
    #[serde(default = "prov")] pub provedor: String,
    #[serde(default)] pub cfg: Cfg,
}
fn prov() -> String { "local".into() }

#[derive(Default)]
pub struct Rt {
    pub contas: Vec<Guardada>,
    pub ws: HashMap<String, JoinHandle<()>>,
    pub live: HashMap<String, ws::Live>,
    pub views: Vec<String>,
    pub layout: String,
    pub foco: Option<String>,
    pub sel: Option<String>,
    pub pids: (Option<u32>, Option<u32>),
    pub abrindo_trade: bool,
    pub versao: Option<String>,
    pub saida: HashMap<String, Vec<serde_json::Value>>,
    pub hunt_cat: HashMap<String, serde_json::Value>,
    pub hunt_rank: HashMap<String, serde_json::Value>,
    pub hunt_status: HashMap<String, String>,
    pub hunt_pedido: std::collections::HashSet<String>,
    pub pontes: HashMap<String, tokio::sync::mpsc::UnboundedSender<String>>,
}
pub type Estado = Arc<Mutex<Rt>>;


fn dados(app: &AppHandle) -> PathBuf {
    let d = app.path().app_data_dir().expect("app_data_dir");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn salvar(app: &AppHandle, rt: &Rt) {
    let p = dados(app).join("contas.json");
    let _ = std::fs::write(&p, serde_json::to_string_pretty(&rt.contas).unwrap());
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)); }
}
pub fn label(nick: &str) -> String {
    format!("v-{}", nick.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>())
}


#[derive(Serialize, Clone)]
struct Vista {
    nick: String,
    modo: String,
    cfg: Cfg,
    #[serde(flatten)]
    live: ws::Live,
}
#[derive(Serialize, Clone)]
struct Snap { trade: bool, versao: Option<String>, contas: Vec<Vista>, layout: String, foco: Option<String>, sel: Option<String>, views: Vec<String> }

fn snapshot(rt: &Rt) -> Snap {
    let contas = rt.contas.iter().map(|c| {
        let mut live = rt.live.get(&c.nick).cloned().unwrap_or_default();
        let ws_on = rt.ws.get(&c.nick).map_or(false, |h| !h.inner().is_finished());
        let pid_role = match c.cfg.role.as_str() { "scanner" => rt.pids.0, "atuador" => rt.pids.1, _ => None };
        let modo = if rt.views.contains(&c.nick) { "janela" }
            else if ws_on { "ws" }
            else if pid_role.is_some() { c.cfg.role.as_str() }
            else { "off" };
        match modo {
            "janela" => { if live.inicio == 0 { live.status = "abrindo janela…".into(); } }
            "scanner" | "atuador" => live.status = format!("{modo} · pid {}", pid_role.unwrap()),
            "off" => { if !c.cfg.role.is_empty() { live.status = format!("{} parou", c.cfg.role); } else if !ws_on { live.status = "parada".into(); } }
            _ => {}
        }
        Vista { nick: c.nick.clone(), modo: modo.into(), cfg: c.cfg.clone(), live }
    }).collect();
    Snap { trade: trade_disponivel(), versao: rt.versao.clone(), contas, layout: rt.layout.clone(), foco: rt.foco.clone(), sel: rt.sel.clone(), views: rt.views.clone() }
}
pub fn empurrar(app: &AppHandle, st: &Estado) {
    let s = snapshot(&st.lock().unwrap());
    let _ = app.emit("contas", s);
}


type Ret = (f64, f64, f64, f64);
fn retangulos(layout: &str, foco: &Option<String>, views: &[String], a: Ret) -> Vec<(String, Option<Ret>)> {
    let (x, y, w, h) = a;
    let n = views.len();
    if n == 0 { return vec![]; }
    let f = foco.clone().filter(|f| views.contains(f)).unwrap_or_else(|| views[0].clone());
    match layout {
        "abas" => views.iter().map(|v| (v.clone(), (*v == f).then_some(a))).collect(),
        "grade" => {
            let cols = (n as f64).sqrt().ceil() as usize;
            let rows = (n + cols - 1) / cols;
            let (cw, ch) = (w / cols as f64, h / rows as f64);
            views.iter().enumerate().map(|(i, v)| (v.clone(), Some((x + (i % cols) as f64 * cw, y + (i / cols) as f64 * ch, cw, ch)))).collect()
        }
        "1n" => {
            if n == 1 { return vec![(views[0].clone(), Some(a))]; }
            let gw = (w * 0.72).floor();
            let resto: Vec<&String> = views.iter().filter(|v| **v != f).collect();
            let rh = h / resto.len() as f64;
            let mut out = vec![(f.clone(), Some((x, y, gw, h)))];
            for (i, v) in resto.iter().enumerate() { out.push(((*v).clone(), Some((x + gw, y + i as f64 * rh, w - gw, rh)))); }
            out
        }
        _ => views.iter().map(|v| (v.clone(), None)).collect(),
    }
}

/// Posiciona/mostra/esconde um webview. No Linux o Tauri empilha os filhos num gtk::Box vertical
/// (posição e tamanho são ignorados), então movemos cada um para um gtk::Fixed, onde valem.
fn colocar(wv: &tauri::Webview, r: Option<Ret>) {
    #[cfg(target_os = "linux")]
    {
        let _ = wv.with_webview(move |w| {
            use gtk::prelude::*;
            let wk = w.inner();
            let wid: &gtk::Widget = wk.upcast_ref();
            let fixed = match wid.parent() {
                Some(p) if p.type_().name() == "GtkFixed" => p.downcast::<gtk::Fixed>().unwrap(),
                Some(p) => {
                    let vbox = p.downcast::<gtk::Container>().unwrap();
                    let fixed = vbox.children().into_iter().find(|c| c.type_().name() == "GtkFixed").and_then(|c| c.downcast::<gtk::Fixed>().ok()).unwrap_or_else(|| {
                        let f = gtk::Fixed::new();
                        if let Ok(b) = vbox.clone().downcast::<gtk::Box>() { b.pack_start(&f, true, true, 0); } else { vbox.add(&f); }
                        f.show();
                        f
                    });
                    vbox.remove(wid);
                    fixed.put(wid, 0, 0);
                    fixed
                }
                None => return,
            };
            match r {
                Some((x, y, w, h)) => {
                    fixed.move_(wid, x as i32, y as i32);
                    wid.set_size_request((w as i32).max(1), (h as i32).max(1));
                    wid.show();
                }
                None => wid.hide(),
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    match r {
        Some((x, y, w, h)) => { let _ = wv.set_position(LogicalPosition::new(x, y)); let _ = wv.set_size(LogicalSize::new(w, h)); let _ = wv.show(); }
        None => { let _ = wv.hide(); }
    }
}

fn relayout(app: &AppHandle, st: &Estado) {
    let Some(win) = app.get_window("main") else { return };
    let (Ok(sz), Ok(sc)) = (win.inner_size(), win.scale_factor()) else { return };
    let sz = sz.to_logical::<f64>(sc);
    if let Some(ui) = app.get_webview("ui") { colocar(&ui, Some((0.0, 0.0, sz.width, sz.height))); }
    let (layout, foco, views) = { let rt = st.lock().unwrap(); (rt.layout.clone(), rt.foco.clone(), rt.views.clone()) };
    let area = (SIDEBAR, BARRA, (sz.width - SIDEBAR).max(10.0), (sz.height - BARRA).max(10.0));
    for (nick, r) in retangulos(&layout, &foco, &views, area) {
        if let Some(wv) = app.get_webview(&label(&nick)) {
            colocar(&wv, r);
        }
    }
}


/// Espia o websocket do jogo: repassa as mensagens ao núcleo (ponte_msg) e aceita comandos
/// (window.__pd_send) para a automação agir pelo mesmo socket, sem abrir uma 2ª sessão.
fn ponte_js(nick: &str) -> String {
    format!(r#"(function(){{if(location.host!=='pokeidle.io'||window.__pd)return;window.__pd=1;
const NICK={};const N=window.WebSocket;let cur=null;
const inv=m=>{{try{{window.__TAURI_INTERNALS__.invoke('ponte_msg',{{nick:NICK,msg:m}})}}catch(e){{}}}};
function W(u,p){{const s=p===undefined?new N(u):new N(u,p);
 if(/pokeidle\.io/.test(String(u))){{cur=s;
  s.addEventListener('message',e=>{{const d=e.data;if(typeof d==='string'&&d.slice(0,60).indexOf('"t":"campo"')<0)inv(d)}});
  s.addEventListener('close',()=>{{if(cur===s){{cur=null;inv('{{"t":"__fechou"}}')}}}});}}
 return s}}
W.prototype=N.prototype;for(const k of['CONNECTING','OPEN','CLOSING','CLOSED'])W[k]=N[k];
window.WebSocket=W;
window.__pd_send=t=>{{if(cur&&cur.readyState===1){{cur.send(t);return true}}return false}};
}})();"#, serde_json::to_string(nick).unwrap())
}

fn abrir_view(app: &AppHandle, st: &Estado, nick: &str) -> Result<(), String> {
    let win = app.get_window("main").ok_or("sem janela principal")?;
    let lb = label(nick);
    if app.get_webview(&lb).is_some() { return Ok(()); }
    let url: tauri::Url;
    let b;
    if nick == TRADE_VIEW {
        url = PAINEL_TRADE.parse().unwrap();
        b = WebviewBuilder::new(&lb, WebviewUrl::External(url));
    } else {
        let c = st.lock().unwrap().contas.iter().find(|c| c.nick == nick).cloned().ok_or("conta não existe")?;
        let sessao = json!({"nick":c.nick,"token":c.token,"provedor":c.provedor,"precisaNick":false}).to_string();
        let inj = format!(
            "{}try{{if(location.host==='pokeidle.io'&&!localStorage.getItem('sessao')){{localStorage.setItem('sessao',{});localStorage.setItem('nick',{});}}}}catch(e){{}}",
            ponte_js(&c.nick), serde_json::to_string(&sessao).unwrap(), serde_json::to_string(&c.nick).unwrap());
        let dir = dados(app).join("perfis").join(&lb);
        let _ = std::fs::create_dir_all(&dir);
        url = URL_JOGO.parse().unwrap();
        b = WebviewBuilder::new(&lb, WebviewUrl::External(url)).data_directory(dir).initialization_script(inj);
    }
    win.add_child(b, LogicalPosition::new(SIDEBAR, BARRA), LogicalSize::new(400.0, 300.0)).map_err(|e| e.to_string())?;
    {
        let mut rt = st.lock().unwrap();
        rt.views.push(nick.to_string());
        if rt.layout == "painel" { rt.layout = "abas".into(); }
        rt.foco = Some(nick.to_string());
    }
    relayout(app, st);
    Ok(())
}
fn fechar_view(app: &AppHandle, st: &Estado, nick: &str) {
    if let Some(w) = app.get_webview(&label(nick)) {

        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            let _ = w.with_webview(|p| {
                use webkit2gtk::WebViewExt;
                let wid = p.inner();
                wid.terminate_web_process();
                unsafe { wid.upcast_ref::<gtk::Widget>().destroy(); }
            });
        }
        let _ = w.close();
    }
    let mut rt = st.lock().unwrap();
    rt.views.retain(|n| n != nick);
    if rt.foco.as_deref() == Some(nick) { rt.foco = rt.views.first().cloned(); }
    if rt.views.is_empty() { rt.layout = "painel".into(); }
}


fn trade_dir() -> PathBuf {
    let p = std::env::var("POKEDECK_TRADE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("../trade"));
    p.canonicalize().unwrap_or(p)
}
/// Scanner/Atuador/Trade dependem do bot `trade/` (Python) do projeto: só existem no Linux e com a pasta presente.
fn trade_disponivel() -> bool { cfg!(unix) && trade_dir().join("coletor.py").exists() && trade_dir().join(".venv/bin/python").exists() }
fn script_do(role: &str) -> &'static str { if role == "scanner" { "coletor.py" } else { "atuador.py" } }
fn pid_de(script: &str) -> Option<u32> {
    let dir = trade_dir();
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let args: Vec<String> = cmd.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        if args.len() >= 2 && args[0].contains("python") && args.iter().any(|a| a.ends_with(script)) && std::fs::read_link(format!("/proc/{pid}/cwd")).ok().as_deref() == Some(dir.as_path()) {
            return Some(pid);
        }
    }
    None
}
fn lancar(script: &str, args: &[&str], env: Option<(&str, &PathBuf)>) -> Result<(), String> {
    let dir = trade_dir();
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{}.log", script.trim_end_matches(".py")))).map_err(|e| e.to_string())?;
    let mut c = Command::new(dir.join(".venv/bin/python"));
    c.arg(script).args(args).current_dir(&dir).stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log);
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; c.process_group(0); }
    if let Some((k, v)) = env { c.env(k, v); }
    let mut child = c.spawn().map_err(|e| format!("não consegui iniciar {script}: {e}"))?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(())
}
fn iniciar_role(app: &AppHandle, c: &Guardada, role: &str) -> Result<(), String> {
    let script = script_do(role);
    if pid_de(script).is_some() { return Err(format!("já existe um {script} rodando (fora do app). Pare-o antes.")); }
    let dir = dados(app).join("roles");
    let _ = std::fs::create_dir_all(&dir);
    let arq = dir.join(format!("{role}.json"));
    std::fs::write(&arq, json!({"nick":c.nick,"token":c.token,"provedor":c.provedor,"precisaNick":false}).to_string()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&arq, std::fs::Permissions::from_mode(0o600)); }
    if pid_de("analise.py").is_none() { lancar("analise.py", &["--watch", "2"], None)?; }
    if role == "scanner" { lancar(script, &[], Some(("POKEIDLE_SCANNER_SESSAO", &arq))) }
    else { lancar(script, &["--live"], Some(("POKEIDLE_SESSAO", &arq))) }
}
fn parar_role(app: &AppHandle, st: &Estado, role: &str) {
    if let Some(pid) = pid_de(script_do(role)) { let _ = Command::new("kill").arg(pid.to_string()).status(); }

    let arq = dados(app).join("roles").join(format!("{role}.json"));
    if let Some(v) = std::fs::read_to_string(&arq).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
        let mut rt = st.lock().unwrap();
        if let (Some(n), Some(tk)) = (v["nick"].as_str(), v["token"].as_str()) {
            if let Some(c) = rt.contas.iter_mut().find(|c| c.nick == n) { c.token = tk.to_string(); }
            salvar(app, &rt);
        }
    }
}

fn parar_tudo_da_conta(app: &AppHandle, st: &Estado, nick: &str) {
    if let Some(h) = st.lock().unwrap().ws.remove(nick) { h.abort(); }
    st.lock().unwrap().pontes.remove(nick);
    fechar_view(app, st, nick);
    let role = st.lock().unwrap().contas.iter().find(|c| c.nick == nick).map(|c| c.cfg.role.clone()).unwrap_or_default();
    if !role.is_empty() {
        parar_role(app, st, &role);
        let mut rt = st.lock().unwrap();
        if let Some(c) = rt.contas.iter_mut().find(|c| c.nick == nick) { c.cfg.role.clear(); }
        salvar(app, &rt);
    }
    let mut rt = st.lock().unwrap();
    let l = rt.live.entry(nick.to_string()).or_default();
    l.status = "parada".into();
}


#[tauri::command]
fn listar(st: State<'_, Estado>) -> Snap { snapshot(&st.lock().unwrap()) }

#[tauri::command]
async fn set_modo(app: AppHandle, st: State<'_, Estado>, nick: String, modo: String) -> Result<(), String> {
    aplicar_modo(app, st.inner().clone(), nick, modo).await
}
async fn aplicar_modo(app: AppHandle, st: Estado, nick: String, modo: String) -> Result<(), String> {
    if (modo == "scanner" || modo == "atuador") && !trade_disponivel() { return Err("Scanner/Atuador precisam do bot trade/ (só no Linux, com a pasta do projeto)".into()); }

    if modo == "scanner" || modo == "atuador" {
        let outro = st.lock().unwrap().contas.iter().find(|c| c.cfg.role == modo && c.nick != nick).map(|c| c.nick.clone());
        if let Some(o) = outro { parar_tudo_da_conta(&app, &st, &o); }
    }
    parar_tudo_da_conta(&app, &st, &nick);
    let res = match modo.as_str() {
        "ws" => { let h = spawn(ws::laco(app.clone(), st.clone(), nick.clone(), false)); st.lock().unwrap().ws.insert(nick.clone(), h); Ok(()) }
        "janela" => abrir_view(&app, &st, &nick).map(|_| {
            let h = spawn(ws::laco(app.clone(), st.clone(), nick.clone(), true));
            st.lock().unwrap().ws.insert(nick.clone(), h);
        }),
        "scanner" | "atuador" => {
            let c = st.lock().unwrap().contas.iter().find(|c| c.nick == nick).cloned().ok_or("conta não existe")?;
            iniciar_role(&app, &c, &modo).map(|_| {
                let mut rt = st.lock().unwrap();
                if let Some(c) = rt.contas.iter_mut().find(|c| c.nick == nick) { c.cfg.role = modo.clone(); }
                salvar(&app, &rt);
            })
        }
        _ => Ok(()),
    };
    if modo != "off" { st.lock().unwrap().sel.get_or_insert(nick.clone()); }
    relayout(&app, &st);
    empurrar(&app, &st);
    res
}

/// Chamado pelo script injetado na janela do jogo: cada mensagem recebida pelo socket do jogo.
#[tauri::command]
fn ponte_msg(st: State<'_, Estado>, nick: String, msg: String) {
    if let Some(tx) = st.lock().unwrap().pontes.get(&nick) { let _ = tx.send(msg); }
}

#[tauri::command]
fn set_cfg(app: AppHandle, st: State<'_, Estado>, nick: String, cfg: Cfg) -> Result<(), String> {
    let st: Estado = st.inner().clone();
    {
        let mut rt = st.lock().unwrap();
        let c = rt.contas.iter_mut().find(|c| c.nick == nick).ok_or("conta não existe")?;
        let role = c.cfg.role.clone();
        c.cfg = cfg;
        c.cfg.role = role;
        salvar(&app, &rt);
    }
    empurrar(&app, &st);
    Ok(())
}

/// Troca a hunt da conta (só no modo WS). Só aceita `hunt.select` com slug do catálogo.
#[tauri::command]
fn set_hunt(app: AppHandle, st: State<'_, Estado>, nick: String, slug: String) -> Result<(), String> {
    let st: Estado = st.inner().clone();
    let mut rt = st.lock().unwrap();
    if !rt.ws.get(&nick).map_or(false, |h| !h.inner().is_finished()) { return Err("ligue o WS desta conta primeiro".into()); }
    let ok = rt.hunt_cat.get(&nick).and_then(|c| c.as_array()).map_or(false, |a| a.iter().any(|h| h[0].as_str() == Some(slug.as_str())));
    if !ok { return Err("hunt desconhecida".into()); }
    rt.saida.entry(nick).or_default().push(json!({"t":"hunt.select","slug":slug}));
    drop(rt);
    let _ = app;
    Ok(())
}
#[tauri::command]
fn recalcular_hunt(st: State<'_, Estado>, nick: String) { st.lock().unwrap().hunt_pedido.insert(nick); }
#[tauri::command]
fn hunt_info(st: State<'_, Estado>, nick: String) -> serde_json::Value {
    let rt = st.lock().unwrap();
    json!({"nick":nick,"catalogo":rt.hunt_cat.get(&nick),"rank":rt.hunt_rank.get(&nick),"status":rt.hunt_status.get(&nick)})
}

#[tauri::command]
fn set_layout(app: AppHandle, st: State<'_, Estado>, modo: String) {
    let st: Estado = st.inner().clone();
    st.lock().unwrap().layout = modo;
    relayout(&app, &st);
    empurrar(&app, &st);
}
#[tauri::command]
fn set_foco(app: AppHandle, st: State<'_, Estado>, nick: String) {
    let st: Estado = st.inner().clone();
    { let mut rt = st.lock().unwrap(); rt.foco = Some(nick.clone()); rt.sel = Some(nick); }
    relayout(&app, &st);
    empurrar(&app, &st);
}
#[tauri::command]
fn set_sel(app: AppHandle, st: State<'_, Estado>, nick: String) {
    let st: Estado = st.inner().clone();
    st.lock().unwrap().sel = Some(nick);
    empurrar(&app, &st);
}

#[tauri::command]
fn add_conta(app: AppHandle, st: State<'_, Estado>, json_sessao: String) -> Result<(), String> {
    let g: Guardada = serde_json::from_str(&json_sessao).map_err(|e| format!("JSON inválido: {e}"))?;
    if g.nick.is_empty() || g.token.is_empty() { return Err("faltou nick ou token".into()); }
    let st: Estado = st.inner().clone();
    {
        let mut rt = st.lock().unwrap();
        rt.contas.retain(|c| c.nick != g.nick);
        rt.contas.push(g);
        salvar(&app, &rt);
    }
    empurrar(&app, &st);
    Ok(())
}

#[tauri::command]
async fn remover(app: AppHandle, st: State<'_, Estado>, nick: String) -> Result<(), String> {
    let st: Estado = st.inner().clone();
    parar_tudo_da_conta(&app, &st, &nick);
    { let mut rt = st.lock().unwrap(); rt.contas.retain(|c| c.nick != nick); rt.live.remove(&nick); salvar(&app, &rt); }
    relayout(&app, &st);
    empurrar(&app, &st);
    Ok(())
}

/// Abre o painel web do trade (sobe o servidor dele se a porta estiver fechada) como mais uma aba.
#[tauri::command]
async fn abrir_trade(app: AppHandle, st: State<'_, Estado>) -> Result<(), String> {
    let st: Estado = st.inner().clone();
    if !trade_disponivel() { return Err("o painel do Trade precisa do bot trade/ (só no Linux)".into()); }
    if st.lock().unwrap().abrindo_trade { return Ok(()); }
    st.lock().unwrap().abrindo_trade = true;
    let r = abrir_trade_int(&app, &st).await;
    st.lock().unwrap().abrindo_trade = false;
    r
}
async fn abrir_trade_int(app: &AppHandle, st: &Estado) -> Result<(), String> {
    let (app, st) = (app.clone(), st.clone());
    if st.lock().unwrap().views.iter().any(|v| v == TRADE_VIEW) {
        { let mut rt = st.lock().unwrap(); rt.foco = Some(TRADE_VIEW.into()); if rt.layout == "painel" { rt.layout = "abas".into(); } }
        relayout(&app, &st); empurrar(&app, &st); return Ok(());
    }
    if std::net::TcpStream::connect_timeout(&"127.0.0.1:8765".parse().unwrap(), Duration::from_millis(300)).is_err() {
        let dir = trade_dir();
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("painel.log")).map_err(|e| e.to_string())?;
        let mut cmd = Command::new("python3");
        cmd.arg("painel/servidor.py").current_dir(&dir).stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log);
        #[cfg(unix)]
        { use std::os::unix::process::CommandExt; cmd.process_group(0); }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        std::thread::spawn(move || { let _ = child.wait(); });
        tokio_sleep().await;
    }
    abrir_view(&app, &st, TRADE_VIEW)?;
    empurrar(&app, &st);
    Ok(())
}
async fn tokio_sleep() { tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(1200))).await.ok(); }

#[tauri::command]
fn fechar_trade(app: AppHandle, st: State<'_, Estado>) {
    let st: Estado = st.inner().clone();
    fechar_view(&app, &st, TRADE_VIEW);
    relayout(&app, &st);
    empurrar(&app, &st);
}

fn main() {

    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::{MemoryPressureSettings, WebsiteDataManager};
        let _ = gtk::init();
        let mut m = MemoryPressureSettings::new();
        m.set_memory_limit(220);
        m.set_poll_interval(5.0);
        m.set_conservative_threshold(0.3);
        m.set_strict_threshold(0.5);
        WebsiteDataManager::set_memory_pressure_settings(&mut m);
    }
    tauri::Builder::default()
        .manage::<Estado>(Arc::new(Mutex::new(Rt { layout: "painel".into(), ..Default::default() })))
        .setup(|app| {
            let st: Estado = app.state::<Estado>().inner().clone();
            let h = app.handle().clone();
            let mut contas: Vec<Guardada> = std::fs::read_to_string(dados(&h).join("contas.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            if contas.is_empty() {
                if let Some(g) = std::fs::read_to_string("../sessao.json").ok().and_then(|s| serde_json::from_str::<Guardada>(&s).ok()) { contas.push(g); }
            }
            { let mut rt = st.lock().unwrap(); rt.contas = contas; salvar(&h, &rt); }

            let win = WindowBuilder::new(app, "main").title("PokeDeck").inner_size(1360.0, 860.0).build()?;
            let ui = win.add_child(WebviewBuilder::new("ui", WebviewUrl::App("index.html".into())), LogicalPosition::new(0.0, 0.0), LogicalSize::new(1360.0, 860.0))?;

            #[cfg(target_os = "linux")]
            let _ = ui.with_webview(|p| {
                use webkit2gtk::{SettingsExt, WebViewExt};
                if let Some(cfg) = p.inner().settings() {
                    cfg.set_enable_page_cache(false);
                    cfg.set_enable_webgl(false);
                    cfg.set_enable_media(false);
                    cfg.set_enable_media_stream(false);
                    cfg.set_enable_webaudio(false);
                    cfg.set_enable_developer_extras(false);
                }
            });
            #[cfg(not(target_os = "linux"))]
            let _ = &ui;
            relayout(&h, &st);
            { let (h2, s2) = (h.clone(), st.clone()); win.on_window_event(move |e| { if let WindowEvent::Resized(_) = e { relayout(&h2, &s2); } }); }


            { let (h2, s2) = (h.clone(), st.clone());
              spawn(async move { loop {
                  let v = tauri::async_runtime::spawn_blocking(|| {
                      match ws::agente().get("https://pokeidle.io/saude").call() { Ok(r) => r.into_json::<serde_json::Value>().ok()?["versao"].as_str().map(String::from), Err(e) => { eprintln!("saude: {e}"); None } }
                  }).await.ok().flatten();
                  if let Some(v) = v {
                      let antiga = { let mut rt = s2.lock().unwrap(); let a = rt.versao.clone(); rt.versao = Some(v.clone()); a };
                      if antiga.as_deref() != Some(v.as_str()) {
                          if antiga.is_some() {
                              eprintln!("jogo atualizou: {antiga:?} -> {v}; recarregando janelas e recalculando /hunt");
                              let (views, nicks) = { let rt = s2.lock().unwrap(); (rt.views.clone(), rt.contas.iter().map(|c| c.nick.clone()).collect::<Vec<_>>()) };
                              for n in views.iter().filter(|n| n.as_str() != TRADE_VIEW) { if let Some(w) = h2.get_webview(&label(n)) { let _ = w.eval("location.reload()"); } }
                              let mut rt = s2.lock().unwrap(); for n in nicks { rt.hunt_pedido.insert(n); }
                          }
                          empurrar(&h2, &s2);
                      }
                  }
                  tokio_sleep_s(60).await;
              } }); }


            { let (h2, s2) = (h.clone(), st.clone());
              spawn(async move { loop {
                  let p = (pid_de("coletor.py"), pid_de("atuador.py"));
                  let mudou = { let mut rt = s2.lock().unwrap(); let m = rt.pids != p; rt.pids = p; m };
                  if mudou { empurrar(&h2, &s2); }
                  tokio_sleep_s(3).await;
              } }); }


            let nicks: Vec<String> = st.lock().unwrap().contas.iter().map(|c| c.nick.clone()).collect();
            if std::env::var("POKEDECK_AUTO_WS").is_ok() {
                for n in &nicks { let hd = spawn(ws::laco(h.clone(), st.clone(), n.clone(), false)); st.lock().unwrap().ws.insert(n.clone(), hd); }
            }
            if let Ok(role) = std::env::var("POKEDECK_AUTO_ROLE") {
                if let Some(n) = nicks.first().cloned() { let (h2, s2) = (h.clone(), st.clone()); spawn(async move { let r = aplicar_modo(h2, s2, n, role).await; eprintln!("auto-role: {r:?}"); }); }
            }
            if std::env::var("POKEDECK_AUTO_VIEW").is_ok() {
                for n in &nicks { let _ = abrir_view(&h, &st, n); }
                if std::env::var("POKEDECK_AUTO_TRADE").is_ok() { let _ = abrir_view(&h, &st, TRADE_VIEW); }
                if std::env::var("POKEDECK_AUTO_CLOSE").is_ok() {
                    let (h2, s2, ns) = (h.clone(), st.clone(), nicks.clone());
                    spawn(async move { tokio_sleep_s(25).await; for n in &ns { fechar_view(&h2, &s2, n); } relayout(&h2, &s2); eprintln!("auto-close feito"); });
                }
                if let Ok(l) = std::env::var("POKEDECK_LAYOUT") { st.lock().unwrap().layout = l; relayout(&h, &st); }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![listar, set_modo, set_cfg, set_hunt, recalcular_hunt, hunt_info, set_layout, set_foco, set_sel, add_conta, remover, abrir_trade, fechar_trade, ponte_msg])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o PokeDeck");
}
async fn tokio_sleep_s(s: u64) { tauri::async_runtime::spawn_blocking(move || std::thread::sleep(Duration::from_secs(s))).await.ok(); }
