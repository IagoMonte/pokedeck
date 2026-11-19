import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { createRequire } from 'node:module';
import { pathToFileURL, fileURLToPath } from 'node:url';

const ORIGEM = 'https://pokeidle.io';
const AQUI = path.dirname(fileURLToPath(import.meta.url));
const RAIZ_CACHE = process.env.POKEDECK_CACHE || path.join(os.homedir(), '.cache', 'pokedeck', 'jogo');
const TOP = 40;

const MODULOS = {
  analyser: 'hunt-analyser.mjs', recompensa: 'shared/recompensa-hunt.mjs', cooldown: 'shared/cooldown-golpes.mjs',
  tm: 'shared/tm-elemental.mjs', bicicletas: 'shared/bicicletas.mjs', outland: 'shared/outland.mjs',
  planilhas: 'shared/golpes-planilhas.mjs', baseOutland: 'shared/herdar-base-outland.mjs',
  evolucoes: 'shared/evolucoes-cruzadas.mjs', ajustes: 'shared/ajustes-especie-espelho.mjs',
  alias: 'shared/alias-item.mjs', vendaNpc: 'shared/venda-npc-item.mjs', especiais: 'shared/golpes-especiais.mjs',
  golpesOrre: 'shared/herdar-golpes-orre.mjs', megas: 'shared/megas.mjs', diaSazonal: 'shared/dia-sazonal.mjs',
  ginasios: 'shared/ginasios.mjs',
};
const OPCIONAIS = new Set(['diaSazonal', 'ginasios']);
const ASSETS = ['creatures', 'creatures-novos', 'creatures-outland-novos', 'creatures-formas', 'creatures-audit-overrides', 'items'];

async function baixar(rel) {
  const r = await fetch(`${ORIGEM}/${rel}`, { headers: { 'user-agent': 'PokeDeck/0.1' } });
  if (!r.ok) throw new Error(`${rel}: HTTP ${r.status}`);
  return r;
}

async function versaoDoJogo() {
  try {
    const t = await (await baixar('app')).text();
    return /app\.js\?v=([\w.]+)/.exec(t)?.[1] ?? null;
  } catch { return null; }
}

/** Baixa um módulo e tudo que ele importa; reescreve os imports relativos para caminhos locais corretos. */
async function espelhar(dir, urlPath, feitos) {
  if (feitos.has(urlPath)) return;
  feitos.add(urlPath);
  const destino = path.join(dir, urlPath);
  let src;
  try { src = await fs.readFile(destino, 'utf8'); } catch { src = null; }
  if (src == null) {
    src = await (await baixar(urlPath.replace(/^\//, ''))).text();
  }
  const dependencias = [];
  const reescrito = src.replace(/(from\s*|import\s*\(\s*|import\s+)(['"])(\.{1,2}\/[^'"]+)\2/g, (m, pre, q, spec) => {
    const alvo = new URL(spec, ORIGEM + urlPath).pathname;
    dependencias.push(alvo);
    let rel = path.posix.relative(path.posix.dirname(urlPath), alvo);
    if (!rel.startsWith('.')) rel = './' + rel;
    return `${pre}${q}${rel}${q}`;
  });
  await fs.mkdir(path.dirname(destino), { recursive: true });
  await fs.writeFile(destino, reescrito);
  for (const d of dependencias) await espelhar(dir, d, feitos);
}

async function prepararCache() {
  await fs.mkdir(RAIZ_CACHE, { recursive: true });
  let versao = await versaoDoJogo();
  if (!versao) {
    const ds = (await fs.readdir(RAIZ_CACHE)).filter((d) => !d.startsWith('.')).sort();
    versao = ds.at(-1);
    if (!versao) throw new Error('sem rede e sem cache do jogo');
  }
  const dir = path.join(RAIZ_CACHE, versao);
  const pronto = path.join(dir, '.pronto');
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, 'package.json'), '{"type":"module"}');
  const feitos = new Set();
  let ok = await fs.access(pronto).then(() => true, () => false);
  if (!ok) {
    for (const [k, rel] of Object.entries(MODULOS)) {
      try { await espelhar(dir, '/' + rel, feitos); }
      catch (e) { if (!OPCIONAIS.has(k)) throw new Error(`não consegui baixar ${rel} (${e.message})`); }
    }
    for (const a of ASSETS) {
      const f = path.join(dir, 'assets', a + '.json');
      await fs.mkdir(path.dirname(f), { recursive: true });
      try { await fs.access(f); } catch { try { await fs.writeFile(f, await (await baixar(`assets/${a}.json`)).text()); } catch (e) { if (a === 'creatures') throw e; } }
    }
    await fs.writeFile(pronto, new Date().toISOString());
    for (const d of await fs.readdir(RAIZ_CACHE)) if (d !== versao && !d.startsWith('.')) await fs.rm(path.join(RAIZ_CACHE, d), { recursive: true, force: true });
  }
  return { dir, versao };
}

async function importarModulos(dir) {
  const out = {};
  for (const [k, rel] of Object.entries(MODULOS)) {
    try { out[k] = await import(pathToFileURL(path.join(dir, rel)).href); }
    catch (e) { if (OPCIONAIS.has(k)) out[k] = null; else throw new Error(`não consegui carregar ${rel} (${e.message})`); }
  }
  return out;
}
const lerJson = async (f) => { try { return JSON.parse(await fs.readFile(f, 'utf8')); } catch { return null; } };

function montarCatalogo(mod, fontes) {
  const lista = [];
  for (const f of fontes.creatures) if (f?.creatures?.length) lista.push(...f.creatures);
  mod.outland.aplicarRemapOutlandLista(lista);
  for (const o of fontes.overrides?.overrides ?? []) {
    const c = lista.find((x) => x.pokeId === o.pokeId);
    if (!c) continue;
    for (const k of ['baseHp', 'baseAtk', 'baseDef', 'baseSpAtk', 'baseSpDef', 'baseSpeed']) if (typeof o[k] === 'number') c[k] = o[k];
    if (Array.isArray(o.attacks)) c.attacks = o.attacks.map((a) => ({ ...a }));
  }
  mod.planilhas.aplicarGolpesDasPlanilhas((id) => lista.filter((c) => c.pokeId === id));
  mod.baseOutland.herdarBaseOutland(lista);
  mod.evolucoes.aplicarCadeiasTruncadas(lista);
  mod.evolucoes.normalizarEvolveLevel600(lista);
  mod.ajustes.aplicarAjustesHuntLevel?.((id) => lista.find((c) => c.pokeId === id));
  const especies = new Map();
  for (const c of lista) {
    if (mod.outland.isEspelhoFantasmaPokeId(c.pokeId)) continue;
    mod.especiais.injetarGolpesEspeciais(c);
    especies.set(c.pokeId, c);
  }
  mod.golpesOrre.herdarGolpesOrre(lista);
  for (const mega of mod.megas.criarEspeciesMega?.(lista) ?? []) especies.set(mega.pokeId, mega);
  return especies;
}

const r0 = (x) => (Number.isFinite(x) ? Math.round(x) : 0);
const r2 = (x) => (Number.isFinite(x) ? Math.round(x * 100) / 100 : 0);
const resumo = (l) => ({
  slug: l.slug, nome: l.nome, nivel: l.nivel, regiao: l.regiao, liberada: l.liberada, atual: !!l.atual, inviavel: !!l.inviavel,
  killsH: r0(l.killsH), lutaS: r2(l.lutaMediaS), xpPokemonH: r0(l.xpPokemonH), xpTreinadorH: r0(l.xpTreinadorH),
  ouroH: r0(l.ouroH), liquidoH: r0(l.liquidoH), pocoesH: r2(l.pocoesH), mortesH: r2(l.mortesH), risco: l.risco,
  custoH: r0((l.custoH?.pocoes ?? 0) + (l.custoH?.revives ?? 0) + (l.custoH?.bolas ?? 0)),
  outland: !!l.outland, tokensH: r2(l.tokensH), fragmentosH: r2(l.fragmentosH),
});
const corte = (lista, n = TOP) => [
  ...lista.filter((l) => l.liberada && !l.inviavel).slice(0, n),
  ...lista.filter((l) => !l.liberada && !l.inviavel).sort((a, b) => a.nivel - b.nivel).slice(0, 5),
].map(resumo);

async function main() {
  const entrada = JSON.parse(await new Promise((res, rej) => { let s = ''; process.stdin.on('data', (d) => (s += d)); process.stdin.on('end', () => res(s)); process.stdin.on('error', rej); }));
  const { welcome, eu } = entrada;
  const pk = (eu.pokemons ?? []).find((p) => p.id === eu.activeId);
  if (!pk) throw new Error('sem pokémon em campo');

  const { dir, versao } = await prepararCache();
  const mod = await importarModulos(dir);
  const [cr, novos, outland, formas, overrides, itensJson] = await Promise.all(ASSETS.map((a) => lerJson(path.join(dir, 'assets', a + '.json'))));
  if (!cr?.creatures?.length) throw new Error('catálogo de espécies (creatures.json) ausente');
  const especies = montarCatalogo(mod, { creatures: [cr, novos, outland, formas], overrides });
  const itens = new Map();
  for (const i of itensJson?.items ?? itensJson ?? []) if (i && i.id != null) itens.set(Number(i.id), i);
  for (const c of welcome.itensCompraveis ?? []) {
    const base = itens.get(Number(c.itemId));
    if (base && Number(c.preco) > 0) itens.set(Number(c.itemId), { ...base, npcPrice: Number(c.preco) });
  }
  const precoBola = new Map((welcome.catalogoBolas ?? []).map((b) => [Number(b.id), Number(b.preco ?? b.price ?? b.npcPrice ?? b.priceGold) || 0]));
  const { analyser, recompensa, cooldown, tm, bicicletas, diaSazonal } = mod;
  const tipoDia = (() => { const t = eu?.diaSazonal?.tipo; return typeof t === 'string' && t && (!diaSazonal?.TIPOS_SAZONAIS || diaSazonal.TIPOS_SAZONAIS.includes(t)) ? t : null; })();

  const Motor = createRequire(import.meta.url)(path.join(AQUI, 'engine.cjs'));
  const r = Motor.analisar(pk, {
    jogo: {
      cooldownGolpeComSpeed: cooldown.cooldownGolpeComSpeed, golpeTmElemental: tm.golpeTmElemental,
      efetividade: analyser.efetividade, xpPorDerrota: recompensa.xpPorDerrota, ouroPorDerrotaHunt: recompensa.ouroPorDerrotaHunt,
      bonusLootPct: analyser.bonusLootPct, multXpPokemon: analyser.multXpPokemon, multXpTreinador: analyser.multXpTreinador,
      fatorPassoDaRaridade: bicicletas.fatorPassoDaRaridade, MS_ONDA: analyser.MS_ONDA, MAX_MOBS: analyser.MAX_MOBS,
      MS_DESLOC_MS: analyser.MS_DESLOC_MS, CD_MOVIMENTO_MEDIO_MS: analyser.CD_MOVIMENTO_MEDIO_MS,
      multDanoSazonal: diaSazonal?.multDanoSazonal, multBuffGinasio: mod.ginasios?.multBuffGinasio,
    },
    especies, hunts: welcome.hunts ?? [],
    consts: {
      tabelaTipos: welcome.tabelaTipos, ampliacaoHunt: welcome.ampliacaoHunt, multHpSelvagem: welcome.multHpSelvagem,
      multDanoSelvagem: welcome.multDanoSelvagem, tiposBoost: welcome.tiposBoost, bicicletas: welcome.bicicletas,
      chanceBossToken: welcome.chanceBossToken, outlandTiers: welcome.outlandTiers,
      chanceFragmentos: [welcome.casas?.fragmento?.chanceDrop, welcome.shinyStone?.chanceDrop, welcome.bicicletas?.fragmento?.chanceDrop],
    },
    eu, itens, precoBola, agora: Date.now(), diaSazonal: tipoDia,
  });
  process.stdout.write(JSON.stringify({
    versaoJogo: versao, geradoEm: Date.now(),
    pokemon: { nome: pk.nome, level: pk.level, tipos: r.heroi.tipos, aoe: r.aoe, zMove: pk.tmElemental ?? null },
    bonusXp: { pokemon: r2(r.bonus.pokemon), treinador: r2(r.bonus.treinador) },
    pocao: r.pocao, limiar: r2(r.limiar), huntAtual: eu.huntSlug ?? null, diaSazonal: r.diaSazonal,
    porXp: corte(r.porXp), porOuro: corte(r.porOuro),
    porOutland: [...r.porOutland.filter((l) => l.liberada && !l.inviavel).slice(0, 20)].map(resumo),
    outland: { rotulo: r.outland.rotulo, mult: r2(r.outland.mult) },
  }));
}
main().catch((e) => { process.stderr.write(String(e.stack || e.message || e).slice(0, 800)); process.exit(1); });
