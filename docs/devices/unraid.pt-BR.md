# Unraid {#unraid}

Um servidor Unraid através de sua API GraphQL: um array que não está
iniciado, discos desativados, faltando ou reportando erros de leitura,
verificações de paridade que encontraram erros ou não rodam há semanas,
pools de cache enchendo, contêineres que deveriam estar rodando e não estão,
e o estado das VMs.

O DumbMonit envia uma chave de API com o papel Viewer no cabeçalho
`x-api-key` para `/graphql`. O papel Viewer lê tudo e não pode alterar nada.
A API é embutida no Unraid a partir da 7.2; na 6.12 a 7.1 ela vem com o
plugin Unraid Connect.

A cada minuto o DumbMonit envia uma consulta obrigatória (versão, array,
discos de paridade, dados e cache) e quatro opcionais, cada uma por conta
própria para que um campo que uma versão mais antiga da API não tem, ou VMs
desligadas, só deixem de fora aquela parte: a verificação de paridade, os
contêineres Docker, as VMs e as notificações não lidas.

!!! warning "Validado apenas contra a documentação do fornecedor — ainda não testado em um sistema real"

    Esta integração foi construída a partir da documentação da API do
    Unraid e do esquema GraphQL publicado da API do Unraid (nomes de
    campos, enumerações, tamanhos em kilobytes), não contra um servidor em
    execução. Toda consulta é uma leitura. Avise-nos o que não funcionar.

## O que ele observa {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_unraid_`.

| Métrica | Rótulos | Significado |
|---|---|---|
| `version_info` | `version` | Valor 1: a versão do Unraid. |
| `array_started` | | 1 quando o array está iniciado. |
| `array_state_info` | `state` | Valor 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Capacidade do array. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1 quando o status do disco é `DISK_OK`. Slots vazios são deixados de fora. |
| `disk_status_info` | `disk`, `role`, `status` | Valor 1: o status do Unraid (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Erros de leitura desde que o array iniciou (a coluna Errors). |
| `disk_temperature_celsius` | `disk`, `role` | Apenas enquanto o disco gira: o Unraid não reporta nenhuma para um disco dormindo. |
| `disk_spinning` | `disk`, `role` | 1 quando o disco está girando. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Tamanho, tamanho do sistema de arquivos e ocupação, para discos com um sistema de arquivos. |
| `disks`, `disks_problem`, `disks_with_errors` | | Contagens. |
| `parity_check_status_info` | `status` | Valor 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Uma verificação em andamento. |
| `parity_check_ok` | | Após uma verificação concluída: 1 sem erros de sincronização, 0 com erros ou quando falhou. Uma verificação cancelada não prova nada e não define nada. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | A última verificação. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Contêineres Docker. |
| `container_running`, `container_autostart_stopped` | `container` | Por contêiner; o segundo é 1 quando um contêiner configurado para iniciar com o array não está rodando. |
| `vms`, `vms_running` | | VMs. |
| `vm_state_info`, `vm_crashed` | `vm` (e `state`) | Por VM. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Notificações não lidas. |
| `registration_ok` | | 1 quando o Unraid não informa nenhum problema de licença, 0 quando o estado da licença é um erro (avaliação expirada, chave que não corresponde ao pendrive, arquivo de chave ausente…). |
| `registration_info` | `state`, `type` | Valor 1: o estado da licença como o Unraid o informa (`trial`, `pro`, `eexpired`, `eguid`…) e o tipo de licença. |
| `trial_expiry_seconds` | | Até a licença de avaliação terminar; somente durante uma avaliação. |
| `os_updates_expiry_seconds` | | Até a licença deixar de ter direito às atualizações do Unraid OS; ausente quando a licença não tem essa data. |
| `uptime_seconds` | | Desde que o servidor iniciou. |
| `cpu_usage_percent` | | Carga do processador, todos os núcleos juntos. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Memória. Conta como usada a que não está disponível: o cache que o Unraid pode devolver não é contado. |
| `swap_used_percent` | | Somente quando o servidor tem swap. |

A licença e a carga do sistema (tempo de atividade, processador, memória) vêm de mais duas consultas opcionais. Uma versão mais antiga da API que não as conhece apenas deixa essas linhas de fora: o restante do monitor não é afetado.

As [regras integradas](../alerting/rules.md#unraid) que se aplicam:

| Regra | O que é | Limite padrão | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| Unraid array stopped | O array Unraid não está iniciado: compartilhamentos, contêineres e VMs que vivem nele estão indisponíveis (`dumbmonit_unraid_array_started`). | < 1 | 10 min | Alerta (`critical`) | 6 h |
| Unraid disk disabled or missing | Um disco Unraid está desativado, faltando ou inválido: o array roda degradado, e mais uma falha perde dados (`dumbmonit_unraid_disk_ok`). | < 1 | 5 min | Alerta (`critical`) | 6 h |
| Unraid disk read errors | Um disco Unraid reportou erros de leitura desde que o array iniciou: pode ser desativado na próxima (`dumbmonit_unraid_disk_errors`). | > 0 | 5 min | Aviso (`warning`) | 1 d |
| Unraid disk too hot | Um disco de array ou paridade Unraid permaneceu acima de 55 °C por quinze minutos (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 min | Aviso (`warning`) | 6 h |
| Unraid parity check found errors | A última verificação de paridade Unraid falhou ou encontrou erros de sincronização: a paridade não corresponde mais aos dados (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 min | Aviso (`warning`) | 1 d |
| Unraid parity check overdue | A última verificação de paridade Unraid tem mais de 40 dias: uma verificação mensal foi perdida (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 d | 1 h | Aviso (`warning`) | 1 d |
| Unraid cache nearly full | Um pool de cache Unraid está mais de 90% cheio: escritas recorrem ao array lento, ou falham (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 min | Aviso (`warning`) | 1 d |
| Unraid container stopped | Um contêiner Docker configurado para iniciar com o array não está rodando (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 min | Aviso (`warning`) | 6 h |
| Unraid VM crashed | O Unraid informa uma máquina virtual como travada (crashed) (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 min | Aviso (`warning`) | 6 h |
| Unraid alert unread | O Unraid emitiu uma notificação da mais alta importância (alert) que ninguém leu: disco com falha, superaquecimento, erro do array… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 min | Aviso (`warning`) | 24 h |
| Unraid array nearly full | O array está mais de 95 % cheio: as gravações nos compartilhamentos vão falhar em breve. Resolve abaixo de 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Aviso (`warning`) | 24 h |
| Unraid memory nearly exhausted | Mais de 95 % da memória está em uso há quinze minutos: o Unraid roda a partir da memória, e contêineres ou VMs são encerrados quando ela acaba. Resolve abaixo de 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 min | Aviso (`warning`) | 6 h |
| Unraid license problem | O Unraid informa um problema de licença (avaliação expirada, chave que não corresponde ao pendrive, arquivo de chave ausente…): o array pode se recusar a iniciar depois da próxima parada (`dumbmonit_unraid_registration_ok`). | < 1 | 30 min | Aviso (`warning`) | 24 h |
| Unraid trial ending | A licença de avaliação termina em menos de três dias: depois disso, o array não inicia mais (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 d | 1 h | Aviso (`warning`) | 24 h |
| Unraid OS updates eligibility ending | A licença deixa de ter direito às atualizações do Unraid OS em menos de 30 dias. O servidor continua funcionando; ele apenas para de receber novas versões (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 d | 1 h | Info (`info`) | 7 d |

## A página do monitor {#the-device-page}

O painel Array diz o que está errado em uma frase, mostra a ocupação do
array, a última verificação de paridade, os contêineres e VMs em execução,
depois lista cada disco — paridade primeiro, depois dados, depois cache —
com seu status, temperatura e ocupação, e os contêineres que deveriam estar
rodando.

## Criar uma chave de API somente leitura para o DumbMonit {#create-a-read-only-api-key-for-dumbmonit}

1. No Unraid 7.2 ou posterior a API é embutida. No Unraid 6.12 a 7.1,
   instale o plugin Unraid Connect da aba Apps primeiro: ele traz a mesma
   API, e nenhum login no Unraid Connect é necessário para uso local.

2. Crie uma chave de API com o papel Viewer, que lê tudo e não pode alterar
   nada: em Settings > Management Access > API Keys, ou no terminal do
   Unraid.

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. Copie a chave. No DumbMonit, informe o endereço do servidor Unraid, por
   exemplo "tower.lan", e cole a chave como API key. Se a interface web é
   servida via HTTPS, escolha https como protocolo.

!!! warning
    O Unraid não reporta temperatura para um disco que está parado, e o
    DumbMonit nunca desperta um: um disco dormindo simplesmente não tem
    temperatura até girar de novo.

## Credenciais {#credentials}

| Credencial | Campos |
|---|---|
| Chave de API | Uma chave com o papel Viewer, enviada como `x-api-key`. |

Endereço: o nome de host ou IP do servidor Unraid, `host:porta`, ou uma URL.
Opções:

- **Protocolo** (`http`) e **Porta** (80): os da interface web. Com SSL
  definido como Strict em Settings > Management Access, use `https`, e o
  endereço `myunraid.net` se for esse o certificado.
- **Aceitar um certificado não verificável**: para um certificado
  autoassinado.
- **Tempo limite por requisição** (15 s).

## Limites conhecidos

- O UPS não é lido: conecte-o via [NUT](nut.md) em vez disso.
- Sem detalhe SMART: o DumbMonit lê o estado, os erros e a temperatura que o Unraid dá para cada disco, não os atributos SMART.
- Não é lido se um contêiner tem uma atualização de imagem pendente.
- Os valores de licença, processador e memória precisam de uma versão recente da API do Unraid. Em uma versão que não os tem, essas linhas e suas regras ficam simplesmente ausentes.

## Solução de problemas {#troubleshooting}

"Unraid refused the API key (401)": a chave está errada ou foi excluída.

"Unraid accepted the API key but refused to answer (403)": a chave não tem
papel, ou um papel mais restrito que Viewer.

Nenhum número de contêiner, VM ou paridade: essa consulta opcional falhou
(VMs desligadas, uma versão mais antiga da API sem o campo); o resto ainda é
lido. O motivo está no log do servidor em nível de debug.
