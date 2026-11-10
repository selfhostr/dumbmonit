# Unraid {#unraid}

Um servidor Unraid através da sua API GraphQL: um array que não está
iniciado, discos desativados, em falta ou a reportar erros de leitura,
verificações de paridade que encontraram erros ou não correm há semanas,
pools de cache a enchê-se, contentores que deveriam estar em execução e não
estão, e o estado das VMs.

O DumbMonit envia uma chave de API com a função Viewer no cabeçalho
`x-api-key` para `/graphql`. A função Viewer lê tudo e não pode alterar
nada. A API está incorporada no Unraid a partir da 7.2; na 6.12 a 7.1 vem
com o plugin Unraid Connect.

A cada minuto, o DumbMonit envia uma consulta obrigatória (versão, array,
paridade, discos de dados e cache) e quatro opcionais, cada uma por si só
para que um campo que uma versão mais antiga da API não tenha, ou VMs
desligadas, só deixe essa parte de fora: a verificação de paridade, os
contentores Docker, as VMs e as notificações não lidas.

!!! warning "Validado apenas contra a documentação do fornecedor — ainda não testado num sistema real"

    Esta integração foi construída a partir da documentação da API do
    Unraid e do esquema GraphQL publicado da API do Unraid (nomes de
    campos, enumerações, tamanhos em kilobytes), não contra um servidor em
    funcionamento. Toda consulta é uma leitura. Diga-nos o que falha.

## O que monitoriza {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_unraid_`.

| Métrica | Etiquetas | Significado |
|---|---|---|
| `version_info` | `version` | Valor 1: a versão do Unraid. |
| `array_started` | | 1 quando o array está iniciado. |
| `array_state_info` | `state` | Valor 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Capacidade do array. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1 quando o estado do disco é `DISK_OK`. Ranhuras vazias são deixadas de fora. |
| `disk_status_info` | `disk`, `role`, `status` | Valor 1: o estado do Unraid (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Erros de leitura desde que o array arrancou (a coluna Errors). |
| `disk_temperature_celsius` | `disk`, `role` | Só enquanto o disco gira: o Unraid não reporta nenhuma para um disco em suspensão. |
| `disk_spinning` | `disk`, `role` | 1 quando o disco gira. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Tamanho, tamanho do sistema de ficheiros e ocupação, para discos com um sistema de ficheiros. |
| `disks`, `disks_problem`, `disks_with_errors` | | Contagens. |
| `parity_check_status_info` | `status` | Valor 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Uma verificação em curso. |
| `parity_check_ok` | | Depois de uma verificação terminada: 1 sem erros de sincronização, 0 com erros ou quando falhou. Uma verificação cancelada não prova nem define nada. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | A última verificação. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Contentores Docker. |
| `container_running`, `container_autostart_stopped` | `container` | Por contentor; o segundo é 1 quando um contentor definido para iniciar com o array não está em execução. |
| `vms`, `vms_running` | | VMs. |
| `vm_state_info`, `vm_crashed` | `vm` (e `state`) | Por VM. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Notificações não lidas. |
| `registration_ok` | | 1 quando o Unraid não indica nenhum problema de licença, 0 quando o estado da licença é um erro (período experimental expirado, chave que não corresponde à pen USB, ficheiro de chave em falta…). |
| `registration_info` | `state`, `type` | Valor 1: o estado da licença tal como o Unraid o indica (`trial`, `pro`, `eexpired`, `eguid`…) e o tipo de licença. |
| `trial_expiry_seconds` | | Até a licença experimental terminar; apenas durante um período experimental. |
| `os_updates_expiry_seconds` | | Até a licença deixar de ter direito às atualizações do Unraid OS; ausente quando a licença não tem essa data. |
| `uptime_seconds` | | Desde o arranque do servidor. |
| `cpu_usage_percent` | | Carga do processador, todos os núcleos em conjunto. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Memória. Conta como usada a que não está disponível: a cache que o Unraid pode devolver não é contada. |
| `swap_used_percent` | | Apenas quando o servidor tem swap. |

A licença e a carga do sistema (tempo de atividade, processador, memória) vêm de mais duas consultas opcionais. Uma versão mais antiga da API que não as conhece apenas deixa essas linhas de fora: o resto do monitor não é afetado.

As [regras incorporadas](../alerting/rules.md#unraid) que se aplicam:

| Regra | O quê | Limiar predefinido | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| Unraid array stopped | O array Unraid não está iniciado: partilhas, contentores e VMs que vivem nele estão indisponíveis (`dumbmonit_unraid_array_started`). | < 1 | 10 min | Warning (`critical`) | 6 h |
| Unraid disk disabled or missing | Um disco Unraid está desativado, em falta ou inválido: o array corre degradado, e mais uma falha perde dados (`dumbmonit_unraid_disk_ok`). | < 1 | 5 min | Warning (`critical`) | 6 h |
| Unraid disk read errors | Um disco Unraid reportou erros de leitura desde que o array arrancou: pode ser desativado na próxima (`dumbmonit_unraid_disk_errors`). | > 0 | 5 min | Advisory (`warning`) | 1 d |
| Unraid disk too hot | Um disco de array ou paridade Unraid manteve-se acima de 55 °C durante quinze minutos (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 min | Advisory (`warning`) | 6 h |
| Unraid parity check found errors | A última verificação de paridade Unraid falhou ou encontrou erros de sincronização: a paridade já não corresponde aos dados (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 min | Advisory (`warning`) | 1 d |
| Unraid parity check overdue | A última verificação de paridade Unraid tem mais de 40 dias: uma verificação mensal foi perdida (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 d | 1 h | Advisory (`warning`) | 1 d |
| Unraid cache nearly full | Uma pool de cache Unraid está mais de 90% cheia: as escritas recuam para o array lento, ou falham (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 min | Advisory (`warning`) | 1 d |
| Unraid container stopped | Um contentor Docker definido para iniciar com o array não está em execução (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 min | Advisory (`warning`) | 6 h |
| Unraid VM crashed | O Unraid indica uma máquina virtual como falhada (crashed) (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| Unraid alert unread | O Unraid emitiu uma notificação da mais alta importância (alert) que ninguém leu: disco avariado, sobreaquecimento, erro do array… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 min | Advisory (`warning`) | 24 h |
| Unraid array nearly full | O array está mais de 95 % cheio: as escritas nas partilhas vão falhar em breve. Resolve abaixo de 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Advisory (`warning`) | 24 h |
| Unraid memory nearly exhausted | Mais de 95 % da memória está em uso há quinze minutos: o Unraid corre a partir da memória, e contentores ou VM são terminados quando ela se esgota. Resolve abaixo de 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 min | Advisory (`warning`) | 6 h |
| Unraid license problem | O Unraid indica um problema de licença (período experimental expirado, chave que não corresponde à pen USB, ficheiro de chave em falta…): o array pode recusar-se a arrancar depois da próxima paragem (`dumbmonit_unraid_registration_ok`). | < 1 | 30 min | Advisory (`warning`) | 24 h |
| Unraid trial ending | A licença experimental termina em menos de três dias: depois disso, o array já não arranca (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 d | 1 h | Advisory (`warning`) | 24 h |
| Unraid OS updates eligibility ending | A licença deixa de ter direito às atualizações do Unraid OS em menos de 30 dias. O servidor continua a funcionar; apenas deixa de receber novas versões (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 d | 1 h | Info (`info`) | 7 d |

## A página do monitor {#the-device-page}

O painel Array diz o que está mal numa frase, mostra a ocupação do array, a
última verificação de paridade, os contentores e VMs em execução, depois
lista cada disco — paridade primeiro, depois dados, depois cache — com o
seu estado, temperatura e ocupação, e os contentores que deveriam estar em
execução.

## Criar uma chave de API só de leitura para o DumbMonit {#create-a-read-only-api-key-for-dumbmonit}

1. No Unraid 7.2 ou posterior a API está incorporada. No Unraid 6.12 a 7.1, instale primeiro o plugin Unraid Connect a partir do separador Apps: traz a mesma API, e não é necessário nenhum início de sessão no Unraid Connect para uso local.

2. Crie uma chave de API com a função Viewer, que lê tudo e não pode alterar nada: em Settings > Management Access > API Keys, ou a partir do terminal do Unraid.

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. Copie a chave. No DumbMonit, introduza o endereço do servidor Unraid, por exemplo «tower.lan», e cole a chave como API key. Se a interface web for servida via HTTPS, escolha https como protocolo.

!!! warning
    O Unraid não reporta nenhuma temperatura para um disco que está parado, e o DumbMonit nunca acorda nenhum: um disco em suspensão simplesmente não tem temperatura até girar de novo.

## Credenciais {#credentials}

| Credencial | Campos |
|---|---|
| Chave de API | Uma chave com a função Viewer, enviada como `x-api-key`. |

Endereço: o nome de anfitrião ou IP do servidor Unraid, `anfitrião:porta`,
ou um URL. Opções:

- **Protocol** (`http`) e **Port** (80): os da interface web. Com SSL
  definido como Strict em Settings > Management Access, use `https`, e o
  endereço `myunraid.net` se for esse o certificado.
- **Accept an unverifiable certificate**: para um certificado autoassinado.
- **Timeout per request** (15 s).

## Limites conhecidos

- A UPS não é lida: ligue-a antes através do [NUT](nut.md).
- Sem detalhe SMART: o DumbMonit lê o estado, os erros e a temperatura que o Unraid dá para cada disco, não os atributos SMART.
- Não é lido se um contentor tem uma atualização de imagem à espera.
- Os valores de licença, processador e memória precisam de uma versão recente da API do Unraid. Numa versão que não os tem, essas linhas e as suas regras ficam simplesmente ausentes.

## Resolução de problemas {#troubleshooting}

«Unraid refused the API key (401)»: a chave está errada ou foi eliminada.

«Unraid accepted the API key but refused to answer (403)»: a chave não tem
função, ou uma função mais restrita do que Viewer.

Nenhum valor de contentor, VM ou paridade: essa consulta opcional falhou
(VMs desligadas, uma versão mais antiga da API sem o campo); o resto
continua a ser lido. O motivo está no registo do servidor ao nível de
depuração.
