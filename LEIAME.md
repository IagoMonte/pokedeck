# PokeDeck

Gerenciador de contas do [pokeidle.io](https://pokeidle.io) (não oficial). Cada conta tem um modo:

- **WS** — só websocket, no "modo economia" do jogo: o servidor continua caçando e você não gasta a RAM de uma janela de jogo
  (~550 MB cada). Mostra jogador, hunt, pokémon ativo e equipe ao vivo, e tem:
  - **Captura** (arremessa bola nos corpos; aprende a chance por espécie e pula as ruins),
  - **XP Share** (troca o pokémon no golpe final),
  - **Autobuy** de bolas, poções e revives (com reserva de coins e teto de gasto por hora),
  - **/hunt**: ranking de hunts por XP/h e ouro/h com as fórmulas do próprio jogo, e seletor para trocar de hunt.
- **Janela** — o jogo embutido num painel isolado por conta, em **Abas / Grade / 1+N**.

Só pode haver **1 sessão por conta**: ligar WS desliga a Janela e vice-versa (e abrir o jogo no navegador derruba o app).
As contas ficam no seu PC (`%APPDATA%\dev.iagomonte.pokedeck\contas.json`); o app só fala com o `pokeidle.io`.

## Instalar (Windows 10/11)

1. Extraia o zip numa pasta e rode **PokeDeck.exe** (mantenha a pasta `hunt` ao lado do `.exe`).
2. Precisa do **WebView2** (já vem no Windows 11 e no Windows 10 atualizado). Se o app abrir em branco, instale o
   "Evergreen Bootstrapper" em <https://developer.microsoft.com/microsoft-edge/webview2/>.
3. O ranking `/hunt` precisa do **Node.js** (<https://nodejs.org>, versão 18 ou mais nova). Sem ele o resto funciona e só o
   `/hunt` mostra um aviso. Na 1ª vez ele baixa os módulos do jogo (~10 MB) para `%USERPROFILE%\.cache\pokedeck`.

## Adicionar uma conta

Entre no jogo pelo navegador, abra o F12 → *Application* → *Local Storage* → `pokeidle.io` e copie o valor de `sessao`
(um JSON `{"nick":…,"token":…,"provedor":…}`). No PokeDeck: **+ conta** → cole → **Adicionar**.
**Nunca compartilhe esse JSON**: ele é a chave da conta.

## Avisos

- O jogo permite automação que siga as regras de quem joga na mão; **você é responsável** pelo que faz. Respeite o limite de
  contas por pessoa do jogo.
- Captura e Autobuy **gastam recursos de verdade** (bolas, poções, coins). Comece com mínimos/alvos pequenos e uma reserva alta.
- Scanner/Atuador e o painel do Trade só existem na versão Linux do autor (dependem de um bot externo) e ficam ocultos aqui.

O motor do `/hunt` é do [BookIdle](https://github.com/PatrickKenji/BookIdle-releases) (MIT, © 2026 Tickelicia); licença em `hunt/LICENSE-BookIdle`.

## Compilar

Rust estável + Node. `cargo build --release` (Windows) → `target/release/pokedeck.exe`. Copie a pasta `hunt/` ao lado do `.exe`.
