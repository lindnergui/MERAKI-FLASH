<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" alt="Ícone do Meraki Flash">
</p>

<h1 align="center">Meraki Flash</h1>

<p align="center">
  Crie pendrives bootáveis do Windows e Linux com uma interface simples,<br>
  moderna e segura.
</p>

<p align="center">
  <a href="https://github.com/lindnergui/MERAKI-FLASH/releases/latest"><img src="https://img.shields.io/github/v/release/lindnergui/MERAKI-FLASH?display_name=tag&amp;sort=semver&amp;style=for-the-badge" alt="Versão mais recente"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/lindnergui/MERAKI-FLASH?style=for-the-badge" alt="Licença MIT"></a>
  <img src="https://img.shields.io/badge/plataformas-Windows%20%7C%20Linux-00bcd4?style=for-the-badge" alt="Windows e Linux">
  <a href="https://github.com/lindnergui/MERAKI-FLASH/actions/workflows/release.yml"><img src="https://github.com/lindnergui/MERAKI-FLASH/actions/workflows/release.yml/badge.svg" alt="Status da release"></a>
</p>

## Download

> [!IMPORTANT]
> Baixe o Meraki Flash somente pela página oficial de
> [Releases do GitHub](https://github.com/lindnergui/MERAKI-FLASH/releases/latest).
> A versão atual é a **v1.0.0**, disponível para computadores x86-64.

| Sistema | Formato | Distribuições | Download oficial |
| --- | --- | --- | --- |
| Linux | AppImage | Formato portátil para a maioria das distribuições | [Baixar AppImage v1.0.0](https://github.com/lindnergui/MERAKI-FLASH/releases/download/v1.0.0/Meraki.Flash_1.0.0_amd64.AppImage) |
| Linux | RPM | Fedora, RHEL, openSUSE e derivados | [Baixar RPM v1.0.0](https://github.com/lindnergui/MERAKI-FLASH/releases/download/v1.0.0/Meraki.Flash-1.0.0-1.x86_64.rpm) |
| Windows | EXE | Windows 10 e Windows 11 | [Baixar instalador v1.0.0](https://github.com/lindnergui/MERAKI-FLASH/releases/download/v1.0.0/Meraki.Flash_1.0.0_x64-setup.exe) |

[Ver todas as versões e notas de lançamento](https://github.com/lindnergui/MERAKI-FLASH/releases)

## Instalação

### Linux — AppImage

O AppImage não exige instalação. Depois de baixá-lo, abra um terminal na pasta
do arquivo e conceda permissão de execução:

```bash
chmod +x Meraki.Flash*.AppImage
./Meraki.Flash*.AppImage
```

Depois da primeira execução, também é possível abrir o arquivo com um clique
duplo pelo gerenciador de arquivos. Para autorizar a gravação no dispositivo, o
sistema precisa ter `pkexec` e um agente de autenticação polkit ativo na sessão
gráfica.

### Linux — RPM

No Fedora, RHEL e distribuições compatíveis, abra um terminal na pasta do
download e execute:

```bash
sudo dnf install ./Meraki.Flash*.rpm
```

No openSUSE:

```bash
sudo zypper install ./Meraki.Flash*.rpm
```

Também é possível abrir o pacote RPM pela loja de aplicativos ou pelo
instalador gráfico da distribuição.

### Windows — EXE

Baixe o arquivo `Meraki.Flash_*_x64-setup.exe`, execute-o e siga as etapas do
instalador NSIS. Depois da instalação, abra o **Meraki Flash** pelo Menu Iniciar.
O Windows exibirá a solicitação do UAC quando o aplicativo precisar acessar o
pendrive para gravação.

## Como usar

1. Escolha se a ISO contém Windows ou Linux.
2. Selecione o arquivo `.iso`.
3. Escolha o pendrive USB removível.
4. Em ISOs do Windows, habilite opcionalmente as otimizações Unattended.
5. Clique em **Gravar pendrive**, confira o dispositivo e confirme a operação.
6. Aguarde a conclusão sem remover o pendrive.

> [!CAUTION]
> A gravação apaga todos os dados do dispositivo selecionado. Confirme o nome,
> a capacidade e o caminho do pendrive antes de continuar.

## Principais recursos

- gravação bit a bit de ISOs Linux híbridas;
- preparação de mídias Windows compatíveis com UEFI usando MBR e FAT32;
- divisão automática de `sources/install.wim` maior que 4 GiB em arquivos SWM;
- geração opcional de `autounattend.xml` para instalações Windows otimizadas;
- detecção restrita a dispositivos USB removíveis;
- revalidação do hardware antes de qualquer escrita destrutiva;
- progresso em tempo real com porcentagem, velocidade e tempo estimado;
- aviso opcional de novas versões ao abrir, sem instalação automática;
- releitura e comparação dos dados após gravar uma ISO Linux;
- interface nativa multiplataforma construída com Tauri 2.

## Segurança da gravação

O fluxo de gravação aplica as seguintes proteções:

1. a descoberta aceita apenas discos físicos USB marcados como removíveis e
   exclui o disco que contém o sistema;
2. a confirmação mostra nome, capacidade e caminho bruto do destino;
3. o helper elevado redescobre o dispositivo e compara caminho, capacidade e
   serial antes de abrir o disco;
4. a ISO é validada antes e depois da elevação e não pode estar armazenada no
   próprio destino;
5. no Linux, os volumes são desmontados antes da primeira escrita; no Windows,
   são bloqueados e desmontados com `DeviceIoControl`;
6. somente o helper iniciado por `pkexec` no Linux ou UAC `runas` no Windows
   abre o dispositivo bruto para escrita;
7. a operação sincroniza o dispositivo ao final e transmite fase, porcentagem,
   velocidade e tempo restante para a interface.

## Motor de gravação

### ISOs Linux

Imagens Linux são gravadas bit a bit no dispositivo bruto em blocos de 4 MiB,
preservando a tabela de partições e o conteúdo da ISO híbrida.

### ISOs Windows e UEFI

O fluxo Windows não depende de `dd`, `mount`, `mkfs`, DISM ou outro executável
externo do sistema operacional:

1. lê UDF e usa ISO9660 como fallback, validando intervalos físicos, nomes
   compatíveis com FAT32 e a presença de `EFI/BOOT/boot*.efi`;
2. se `sources/install.wim` exceder 4 GiB, extrai e divide o WIM em
   `install.swm`, `install2.swm` e partes subsequentes antes de tocar no USB;
3. no Linux, o split usa a `libwim` 1.14.4 compilada junto do aplicativo; no
   Windows, usa `WIMCreateFile` e `WIMSplitFile` da `wimgapi.dll` do `System32`;
4. revalida a identidade física do pendrive, desmonta ou bloqueia seus volumes
   e limpa assinaturas de partição antigas;
5. cria uma tabela MBR com uma partição FAT32 LBA ativa, alinhada em 1 MiB, e a
   formata com clusters de 32 KiB;
6. extrai a árvore da ISO diretamente para o FAT32, substituindo somente o WIM
   grande pelos SWMs validados;
7. quando solicitado, grava o `autounattend.xml` validado na raiz do FAT32 e
   sincroniza o dispositivo ao final.

As limitações intencionais atuais são dispositivos de até 2 TiB e a rejeição
de `install.esd` maior que 4 GiB, pois um ESD sólido não pode ser dividido com
a mesma segurança. ISOs que já contêm `install.swm` são aceitas.

Para dividir um WIM, o computador precisa ter espaço temporário equivalente a
aproximadamente duas vezes o tamanho de `install.wim`, mais uma margem de
segurança. O Meraki Flash verifica esse espaço antes da extração. No Linux, ele
prefere `/var/tmp` para evitar o `tmpfs` normalmente usado em `/tmp`, e depois
tenta o diretório temporário do sistema e a pasta da ISO.

### Otimizações Unattended

Para Windows 10 e 11 x64, a interface pode gerar um `autounattend.xml` com as
seguintes opções:

- ignorar verificações de TPM 2.0, Secure Boot e RAM;
- criar uma conta local offline;
- reduzir as perguntas de privacidade e telemetria;
- desativar o Edge em segundo plano;
- ocultar a pesquisa da barra de tarefas;
- remover pacotes conhecidos do Copilot.

O frontend valida e escapa os valores informados. O processo principal envia o
XML opcional ao helper elevado, que revalida tamanho e estrutura, rejeita
DTD/DOCTYPE e permite o recurso apenas no fluxo Windows. A conta local é criada
como Administrador sem senha inicial; defina uma senha segura no primeiro
acesso.

## Desenvolvimento

O projeto usa Tauri 2, Rust, React, TypeScript e Tailwind CSS 4.

### Pré-requisitos de compilação

- Node.js 20 ou mais recente;
- Rust 1.95 ou mais recente;
- dependências nativas do Tauri para Windows ou Linux;
- no Linux, compilador C, `make`, Autotools e `libclang` para compilar a
  biblioteca WIM incluída no projeto.

No Fedora, instale os pacotes necessários:

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel clang-devel \
  autoconf automake libtool make
sudo dnf group install "c-development"
```

Em seguida:

```bash
git clone https://github.com/lindnergui/MERAKI-FLASH.git
cd MERAKI-FLASH
npm ci
npm run tauri dev
```

### Estrutura do projeto

```text
src/                         Interface React
  App.tsx                    Fluxo visual em quatro etapas
  components/UnattendPanel.tsx
                             Configurador visual da instalação automática
  lib/unattend.ts            Validação e template do autounattend.xml
  styles.css                 Design system Meraki/Tailwind
src-tauri/
  src/lib.rs                 Bootstrap e registro dos comandos Tauri
  src/commands/usb.rs        Ponte entre o Tauri e o domínio
  src/commands/flash.rs      Comando que inicia a gravação
  src/flash/                 Helper elevado e motores Linux/Windows
    windows_iso.rs           ISO/UDF, MBR, FAT32, extração e validações
    wim.rs                   Split WIM: wimlib no Linux e WIMGAPI no Windows
  src/elevation.rs           Integração com pkexec e UAC/runas
  crates/meraki-flash-core/  Descoberta testável de volumes removíveis
  crates/wimlib-sys/         Binding e fonte da wimlib para Linux
  Cargo.toml                 Dependências Rust multiplataforma
  capabilities/default.json Permissões mínimas da janela
```

No Linux, a `libwim` incluída é compilada sem `ntfs-3g`, sob a opção LGPL-3.0+
oferecida pelo projeto upstream. Antes de redistribuir binários modificados,
revise as obrigações da LGPL e o aviso do binding em
`src-tauri/crates/wimlib-sys`.

### Testes

```bash
npm test
cargo test --locked --workspace --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --workspace --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run build
```

Os testes automatizados gravam somente em arquivos temporários, incluindo uma
imagem esparsa de 3 GiB usada para validar MBR e FAT32. Nunca use um disco real
como alvo de teste sem uma bancada dedicada e dados descartáveis.

## Lançamentos e CI/CD

O workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) é
acionado quando uma tag SemVer correspondente à versão de
`src-tauri/tauri.conf.json` é enviada ao GitHub.

Cada lançamento executa dois builds independentes:

- **Ubuntu 22.04:** gera AppImage e RPM;
- **Windows Runner:** gera o instalador EXE com NSIS.

Cada build executa os testes e o Clippy antes de gerar instaladores. A release
fica em rascunho até ambos os sistemas passarem e os três artefatos estarem
anexados; só então é publicada automaticamente. A release estável atual usa a tag `v1.0.0`. Para
publicar a próxima correção, depois de atualizar a versão do projeto para
`1.0.1`, por exemplo:

```bash
git tag -a v1.0.1 -m "Meraki Flash v1.0.1"
git push origin v1.0.1
```

O instalador Windows ainda não possui assinatura digital. Para distribuição em
larga escala, configure um certificado de assinatura de código no pipeline.

## Licença

Distribuído sob a licença MIT. Consulte o arquivo [LICENSE](LICENSE).

## Avisos de atualização

A partir da v1.0.0, ao abrir o aplicativo ele consulta a última release estável
pública no GitHub, com limite de cinco segundos. O aviso pode ser fechado em
**Agora não** e a consulta pode ser desativada em **Avisar sobre novas versões ao
abrir**. A preferência fica salva neste computador. Não há download ou instalação
automática, e falhas de rede não interrompem o uso.

A v0.1.0 já distribuída não contém esse recurso: seus usuários precisam instalar
a v1.0.0 manualmente uma vez para receber avisos de lançamentos futuros.

### Regressão visual opcional

Com `npm run preview -- --port 1420` em execução, instale `playwright` sem salvar
no manifesto (`npm install --no-save playwright`), instale seu Chromium no Linux
(`npx playwright install chromium`) e execute `node tests/interface.mjs`. No
Windows o teste usa o Edge instalado. A simulação não chama o motor nativo nem
acessa discos físicos. Ela cobre quatro larguras de janela, etapas, autorização,
erros, bloqueios e a preferência de atualização. A captura fica em `.qa/`.
