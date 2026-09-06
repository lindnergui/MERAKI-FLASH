# Meraki Flash v1.0.0

Disponível para Windows 10/11 x64 (instalador EXE) e Linux x86-64 (AppImage e RPM).

- Interface sem o selo Alpha e sem a mensagem Modo de proteção ativo.
- Novo título: Seu sistema, pronto para uso.
- Etapas numeradas conforme as escolhas, com a quarta marcada durante a gravação.
- A mensagem de autorização desaparece assim que o helper elevado assume a operação.
- Colunas resistentes a nomes longos de ISO, prevenção de cliques duplicados e bloqueio de fechamento durante a operação.
- Aviso opcional de novas versões ao abrir, com botão Agora não e preferência para desativar consultas. Sem instalação automática.
- Correção da validação de ISO armazenada no próprio destino no Windows.
- Escritas MBR/FAT32 alinhadas a setores de 512 ou 4096 bytes.
- Correção de pendrives com ISO9660 que apareciam incorretamente como protegidos no Linux e ajuste da elevação no AppImage.
- Validação da tabela híbrida antes de gravar ISO Linux e releitura para comparar o resultado com a origem.
- Testes automatizados e análise Rust em Windows e Linux antes da publicação dos três instaladores.

**Atualização da v0.1.0:** instale esta versão manualmente. O aviso de novas versões passa a funcionar a partir da v1.0.0.

Os testes de gravação usam arquivos temporários e dispositivos simulados; não substituem testes de inicialização em hardware físico. O instalador Windows permanece sem assinatura digital.
