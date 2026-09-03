# Meraki Flash

Base inicial do aplicativo desktop multiplataforma para criação de pendrives
bootáveis. O projeto usa Tauri 2, Rust, React, TypeScript e Tailwind CSS 4.

## Inicialização

### Pré-requisitos

- Node.js 20 ou mais recente
- Rust 1.95 ou mais recente
- dependências de sistema do Tauri para Windows ou Linux
- no Linux: compilador C, `make`, Autotools e `libclang` para compilar a
  biblioteca WIM incluída no projeto

No Fedora, instale primeiro os pacotes recomendados pelo Tauri:

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel clang-devel \
  autoconf automake libtool make
sudo dnf group install "c-development"
```

```bash
npm install
npm run tauri dev
```

Para criar um projeto equivalente do zero com o gerador oficial:

```bash
npm create tauri-app@latest
```

No assistente, escolha `TypeScript / JavaScript`, seu gerenciador de pacotes,
`React` e `TypeScript`. Depois adicione o plugin de diálogo e o Tailwind:

```bash
npm run tauri add dialog
npm install tailwindcss @tailwindcss/vite
```

## Estrutura

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
  crates/wimlib-sys/         Binding auditado e fonte wimlib para Linux
  Cargo.toml                 Dependências Rust multiplataforma
  capabilities/default.json Permissões mínimas da janela
```

## Gravação de ISOs Linux e Windows

O botão **Gravar pendrive** inicia uma operação destrutiva real para imagens ISO
Linux híbridas e mídias de instalação Windows UEFI. Todo o conteúdo do
dispositivo selecionado será substituído.

O fluxo atual aplica as seguintes proteções:

1. a descoberta aceita apenas discos físicos USB marcados como removíveis e
   exclui o disco que contém o sistema;
2. a confirmação mostra nome, capacidade e caminho bruto do destino;
3. o helper elevado redescobre o dispositivo e compara caminho, capacidade e
   serial antes de abrir o disco;
4. a ISO é validada antes e depois da elevação e não pode estar armazenada no
   próprio destino;
5. no Linux, os volumes são desmontados antes da primeira escrita; no Windows,
   são bloqueados e desmontados com `DeviceIoControl`;
6. somente o helper executado via `pkexec` (Linux) ou UAC `runas` (Windows) abre
   o dispositivo bruto para escrita;
7. a operação sincroniza o dispositivo ao final e transmite fase, porcentagem,
   velocidade e tempo restante pelo sistema de eventos do Tauri.

No Linux, o sistema precisa fornecer `pkexec` e um agente de autenticação polkit
ativo na sessão gráfica. No Windows, o prompt do UAC é usado automaticamente.

### Linux

Imagens Linux são gravadas bit a bit no dispositivo bruto em blocos de 4 MiB,
preservando a tabela de partições e o conteúdo da ISO híbrida.

### Windows/UEFI

O fluxo Windows não depende de `dd`, `mount`, `mkfs`, DISM ou outro executável
externo do sistema operacional:

1. lê UDF e usa ISO9660 como fallback, valida os intervalos físicos dos arquivos,
   nomes compatíveis com FAT32 e a presença de `EFI/BOOT/boot*.efi`;
2. se `sources/install.wim` exceder 4 GiB, extrai e divide o WIM em
   `install.swm`, `install2.swm` etc. antes de tocar no USB;
3. no Linux, o split usa `libwim` 1.14.4 compilada no aplicativo; no Windows,
   usa diretamente `WIMCreateFile` e `WIMSplitFile` da `wimgapi.dll` do
   `System32`;
4. revalida a identidade física do pendrive, desmonta/bloqueia seus volumes e
   limpa as assinaturas de partição antigas;
5. cria uma tabela MBR com uma partição FAT32 LBA ativa, alinhada em 1 MiB, e
   formata-a com clusters de 32 KiB;
6. extrai a árvore da ISO diretamente para o FAT32, substituindo somente o WIM
   grande pelos SWMs válidos;
7. quando solicitado, grava o `autounattend.xml` validado na raiz do FAT32 e
   sincroniza o dispositivo ao final.

As limitações intencionais atuais são: dispositivos de até 2 TiB e rejeição de
`install.esd` maior que 4 GiB, pois ESD sólido não pode ser dividido com a mesma
segurança. ISOs que já contêm `install.swm` são aceitas. Para dividir um WIM, o
host precisa ter espaço temporário para o WIM extraído e suas partes — cerca de
duas vezes o tamanho de `install.wim`, mais uma margem de segurança. O Meraki
faz essa verificação antes da extração; no Linux prefere `/var/tmp` para evitar
o `tmpfs` normalmente usado em `/tmp`, e depois tenta o diretório temporário do
sistema e a pasta da ISO.

### Otimizações Unattended

Depois de selecionar Windows, uma ISO e o pendrive, a interface pode gerar um
`autounattend.xml` para Windows 10/11 x64. O perfil permite ignorar as
verificações de TPM 2.0, Secure Boot e RAM, criar uma conta local, reduzir as
perguntas de privacidade, desativar o Edge em segundo plano, ocultar a pesquisa
da barra de tarefas e remover os pacotes conhecidos do Copilot.

O frontend valida as opções e escapa os valores inseridos. O processo principal
envia o conteúdo opcional ao helper elevado, que revalida tamanho e estrutura
XML, rejeita DTD/DOCTYPE e só aceita esse recurso no fluxo Windows. A conta local
é criada como Administrador sem senha inicial; defina uma senha segura no
primeiro acesso.

No Linux, a `libwim` incluída é compilada sem `ntfs-3g`, sob a opção LGPL-3.0+
oferecida pelo projeto upstream. Antes de distribuir binários, revise as
obrigações de redistribuição e relink da LGPL e o aviso do binding em
`src-tauri/crates/wimlib-sys`.

## Testes

```bash
cargo test --offline --manifest-path src-tauri/Cargo.toml
cargo clippy --offline --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run build
```

Os testes automatizados gravam somente em arquivos temporários, incluindo uma
imagem esparsa de 3 GiB usada para validar MBR + FAT32. Nunca use um disco real
como alvo de teste sem uma bancada dedicada e dados descartáveis.

## Lançamentos

O workflow `.github/workflows/release.yml` compila os instaladores oficiais
quando uma tag SemVer correspondente à versão de `src-tauri/tauri.conf.json` é
enviada ao GitHub. Por exemplo, para a versão `0.1.0`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

O GitHub Actions gera um AppImage e um RPM no Ubuntu 22.04, além de um
instalador EXE com NSIS no Windows, e anexa os três arquivos a uma GitHub
Release pública. O instalador Windows ainda não é assinado digitalmente; para
uma distribuição ampla, configure um certificado de assinatura de código antes
do lançamento estável.
