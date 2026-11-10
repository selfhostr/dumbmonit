# TrueNAS {#truenas}

Servidor de armazenamento ZFS (TrueNAS SCALE, agora TrueNAS Community
Edition): a saúde de cada pool e o disco que falhou, scrubs e resilvers,
utilização de pools e datasets contra as suas quotas, snapshots e
replicação, temperatura dos discos e autotestes SMART, os alertas que o
próprio TrueNAS levanta, serviços e a máquina.

!!! warning "Testado apenas contra uma instância de teste"

    Esta integração foi construída e verificada contra o TrueNAS
    25.04.2.6, instalado a partir da ISO oficial numa máquina virtual para
    esse fim — um pool RAIDZ1, datasets, tarefas de snapshot e
    replicação, depois o mesmo pool com um disco levado offline — não
    contra um NAS que guarda as fotos de uma casa há anos. Uma máquina
    virtual não tem temperaturas de disco nem resultados SMART, por isso
    esses seguem o código-fonte do TrueNAS em vez de uma resposta
    observada. Toda chamada é uma leitura. Diga-nos o que falha.

## A falha para a qual isto existe {#the-failure-this-is-for}

Um vdev mirror ou RAIDZ que perde um disco continua a servir os dados. A
partilha fica montada, as cópias de segurança continuam a correr, a
interface web de toda outra ferramenta continua verde — e o pool agora
não tem nenhuma redundância restante. O segundo disco leva o pool.
`truenas_pool_healthy` cai para 0 no primeiro, a regra **NAS pool
degraded** dispara, e a página do monitor nomeia o disco.

## A página do monitor {#the-device-page}

Além dos gráficos genéricos, um monitor TrueNAS mostra três painéis,
lidos do que a verificação guardou — abrir a página nunca consulta o
próprio NAS:

* **Pools and disks** — cada pool com o seu estado numa palavra, a
  própria frase do ZFS quando algo está mal, quão cheio está, a sua
  disposição (mirror, RAIDZ, cache, log), as suas contagens de erros de
  leitura, escrita e checksum, e sobretudo o nome de qualquer disco que
  não está `ONLINE`. Um scrub ou resilver em curso mostra o seu
  progresso. Depois cada disco: modelo, número de série, tipo, tamanho,
  pool, temperatura e o resultado do seu último autoteste SMART.
* **Datasets and protection** — cada dataset com a sua utilização contra
  a sua quota, a sua contagem de snapshots e quando uma tarefa de
  snapshot o cobriu por último; o último scrub de cada pool e se está em
  atraso; cada tarefa de replicação e snapshot periódico com o seu
  estado, a sua última execução, o último snapshot que tratou e, quando
  falhou, a própria frase de erro do TrueNAS.
* **NAS health** — os alertas que o próprio TrueNAS levantou, os mais
  severos primeiro, depois os serviços parados, depois a máquina:
  versão, tempo de atividade, carga, memória, CPU.

## O que monitoriza {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_truenas_`.

| Família | Métricas | Etiquetas |
|---|---|---|
| Identidade | `version_info` (a versão, sem o prefixo `TrueNAS-SCALE-` que versões mais antigas colocam na frente) | `version` |
| Máquina | `uptime_seconds`, `load1/5/15`, `memory_total_bytes`, `cpu_count` | |
| Pools | `pool_healthy` (o próprio veredicto do ZFS: 0 em `DEGRADED` como em `FAULTED`), `pool_warning` (resilvering, funcionalidades não ativadas), `pool_status_info`, `pool_size/allocated/free_bytes`, `pool_used_percent`, `pool_fragmentation_percent` | `pool`, e `status` para a série info |
| Dispositivos de pool | `pool_device_errors` (acumulados desde o último `zpool clear`), `pool_devices_unhealthy` (ambas ausentes quando o pool não pode ser aberto) | `pool`, `kind` (`read`, `write`, `checksum`) |
| Scrubs | `pool_scan_running`, `pool_scan_percent` (só enquanto em execução), `pool_last_scrub_age_seconds`, `pool_last_scrub_errors`, `pool_scrub_threshold_days` | `pool`, `function` (`SCRUB`, `RESILVER`) |
| Datasets | `dataset_used_bytes`, `dataset_available_bytes`, `dataset_quota_bytes` e `dataset_quota_used_percent` (ausentes sem quota), `dataset_snapshots`, `dataset_snapshot_age_seconds` (idade do último snapshot feito por uma tarefa periódica que cobre o dataset; ausente quando nenhuma o faz), `dataset_locked` (só datasets encriptados) | `dataset`, `pool` |
| Snapshots | `snapshots` (total) | |
| Discos | `disk_temperature_celsius`, `disk_size_bytes`, `disk_smart_failed` (ausente para um disco nunca testado) | `disk`, `serial` |
| Alertas | `alerts` — o número de alertas ativos por nível, zero incluído | `level` (`INFO` … `EMERGENCY`) |
| Tarefas | `replication_error`, `replication_last_run_age_seconds`, `snapshot_task_error`, `snapshot_task_last_run_age_seconds` (só tarefas ativadas) | `task` |
| Serviços | `service_running` (só serviços definidos para iniciar com o NAS) | `service` |
| Coleta | `up`, `scrape_errors`, `scrape_duration_seconds` | |

Algumas coisas que o TrueNAS faz e que vale a pena conhecer:

* **O último scrub só vive no registo de varrimento do pool.** Não há
  histórico de scrub na API, e um resilver sobrescreve o registo de
  varrimento: depois de um resilver, a data do último scrub é
  simplesmente desconhecida até o próximo terminar, e
  `pool_last_scrub_age_seconds` fica ausente em vez de errada.
* **Um pool que não pode ser aberto** — exportado, ou com demasiados
  discos em falta — continua a aparecer, sem disposição e sem tamanhos. O
  seu `pool_healthy` é 0; as séries de dispositivo e tamanho estão
  ausentes em vez de zero.
* **Uma quota não definida não é uma quota de zero.** O TrueNAS responde
  `parsed: null` para «sem quota» e o DumbMonit não publica nenhuma série
  de quota para esse dataset. `quota` tem precedência sobre `refquota`
  quando ambas estão definidas.
* **As contagens de snapshots vêm da própria cache do ZFS**
  (`snapshots_count`), e o total da contagem rápida de snapshots do
  TrueNAS — nenhum snapshot é alguma vez listado. O total é maior do que
  a soma das contagens por dataset: inclui os snapshots dos datasets
  internos do TrueNAS, que a lista de datasets escondem.
* **A idade do snapshot mais recente de um dataset** é a última
  execução da tarefa de snapshot periódico que o cobre, diretamente ou
  através de um pai recursivo. O TrueNAS não pode dar a data do snapshot
  mais recente sem os listar todos, por isso um dataset que nenhuma
  tarefa cobre não tem idade — o que já é informação útil.
* **As temperaturas são lidas da cache do TrueNAS** (`only_cached`),
  atualizada pelo TrueNAS a cada cinco minutos aproximadamente. Um disco
  em suspensão nunca é despertado, e um disco sem leitura não tem
  nenhuma série.
* **Nada é alguma vez iniciado.** Nenhum scrub, nenhum teste SMART,
  nenhuma verificação de atualização, nenhuma execução de replicação. A
  verificação lê.
* **Os caminhos que mudaram entre versões são opcionais.** `zfs/snapshot`
  tornou-se `pool/snapshot` na 25.10 e ambos são tentados;
  `smart/test/results` desapareceu na 25.10 e é então ignorado
  silenciosamente.

As regras incorporadas que se aplicam: Monitor unreachable, NAS pool
degraded, NAS disk errors, NAS pool almost full, NAS scrub found errors,
NAS scrub overdue, NAS disk failed its SMART test, NAS disk too hot, NAS
dataset near its quota, NAS replication failed, NAS snapshot task failed,
NAS snapshots stale, TrueNAS alert raised, NAS service stopped. As
notificações nomeiam o pool, disco, dataset, tarefa ou serviço em causa.

## O que preparar no TrueNAS {#what-to-prepare-in-truenas}

Os passos abaixo são os que o aviso junto ao formulário mostra. Uma coisa
a saber primeiro: através da API REST que o DumbMonit usa, **o TrueNAS
não tem nenhuma chave só de leitura**. As suas funções só de leitura só
foram ligadas à sua API WebSocket mais recente; uma chave cujo
utilizador é um Read-Only Administrator é recusada, com um `403` vazio,
em cada chamada REST. Isto foi verificado no TrueNAS 25.04.2.6: uma chave
Read-Only Administrator recebeu `403` em todos os 55 caminhos REST
testados, e a mesma chave funcionou através da API WebSocket. A chave
tem de pertencer a um utilizador com o privilégio Local Administrator —
por isso recebe um utilizador e um grupo próprios, e nada mais.

1. Na interface web: Credentials → Groups → Add. Dê o seguinte nome ao grupo e dê-lhe o privilégio Local Administrator.

    ```
    dumbmonit
    ```

2. Porquê Local Administrator e não Read-Only Administrator: o TrueNAS só ligou as suas funções só de leitura à sua API WebSocket. Através da API REST que o DumbMonit usa, uma chave só de leitura é recusada em cada chamada. O próprio DumbMonit só lê: nunca inicia um scrub, um teste SMART, uma atualização ou uma replicação.

3. Credentials → Users → Add. Dê o seguinte nome à conta, torne dumbmonit o seu grupo primário, e deixe o acesso shell, sudo e SSH desligados: nenhum deles é necessário.

    ```
    dumbmonit
    ```

4. Crie a chave: Credentials → Users → API Keys → Add, escolha o utilizador dumbmonit e dê um nome à chave. O TrueNAS mostra a chave só uma vez: copie-a. No TrueNAS 24.10 o caminho é o menu do utilizador no canto superior direito → API Keys → Add, e uma chave não pertence a nenhum utilizador.

5. No DumbMonit, introduza o endereço do NAS, por exemplo «nas.lan», e cole a chave. Mantenha HTTPS: o TrueNAS revoga uma chave que recebe uma vez via HTTP simples de outra máquina.

!!! warning

    Esta chave pode alterar qualquer coisa no NAS: o TrueNAS não oferece nenhuma chave só de leitura através da sua API REST. Trate-a como a senha de uma conta que pode apagar os seus pools, reserve-a só para o DumbMonit e revogue-a na mesma página se for exposta. O TrueNAS também vem com um certificado autoassinado: se a ligação for recusada por esse motivo, marque «Accept an unverifiable certificate» nas opções.

Endereço: `192.168.1.20`, `nas.lan`, `nas.lan:8443`, `[fd00::20]` ou um URL
completo. O esquema `https` e a porta 443 são adicionados se estiverem em
falta.

O TrueNAS anunciou que a API REST será removida no TrueNAS 26, a favor
da API WebSocket. Até então, REST é o que toda versão até à 25.10 serve.

## Opções {#options}

| Chave | Rótulo | Predefinição | Ajuda |
|---|---|---|---|
| `scheme` | Protocol | `https` | Mantenha HTTPS: o TrueNAS revoga uma chave de API que recebe via HTTP simples de outra máquina. |
| `port` | Web interface port | `443` | Usada se o endereço não der uma porta. |
| `insecure_tls` | Accept an unverifiable certificate | `false` | O TrueNAS vem com um certificado autoassinado por predefinição. |
| `request_timeout_seconds` | Timeout per request (seconds) | `20` | Tempo permitido para cada chamada de API, de 1 a 120. |
| `datasets` | Watch the datasets | `true` | Utilização contra quota, contagem de snapshots, última alteração de snapshot. |
| `disks` | Watch the disks | `true` | Inventário e temperaturas em cache; um disco em suspensão nunca é despertado. |
| `smart` | Read the SMART test results | `true` | O último autoteste de cada disco; nenhum é alguma vez iniciado. |
| `alerts` | Read TrueNAS's own alerts | `true` | A lista de alertas que o próprio TrueNAS mantém, alertas dispensados deixados de fora. |
| `tasks` | Watch replication and snapshot tasks | `true` | Estado, última execução e último snapshot de cada tarefa. |
| `services` | Watch the services | `true` | Serviços definidos para iniciar com o NAS, e se estão em execução. |

## Regras de alerta incorporadas

Estas regras vêm com o DumbMonit e estão ativas desde o início. Cada uma pode ser editada ou desativada em Alertas; a [referência das regras incorporadas](../alerting/rules.md#truenas) lista todas as regras de todas as integrações.

| Regra | O quê | Limiar predefinido | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| NAS pool degraded | O ZFS já não considera uma pool saudável (`dumbmonit_truenas_pool_healthy`). Uma pool degradada continua a servir os seus dados, que é exatamente por isso que ninguém repara até o próximo disco falhar. A página do monitor nomeia o disco. | < 1 | 5 min | Warning (`critical`) | 6 h |
| NAS disk errors | Discos de uma pool reportaram erros de leitura, escrita ou checksum desde o último `zpool clear` (`dumbmonit_truenas_pool_device_errors`, uma série por tipo). | > 0 | 15 min | Advisory (`warning`) | 24 h |
| NAS pool almost full | Uma pool está mais de 85% cheia (`dumbmonit_truenas_pool_used_percent`). O ZFS abranda bem antes de ficar cheio. Limpa-se aos 80%. | > 85 % | 30 min | Advisory (`warning`), agrava depois de 7 d | 24 h |
| NAS scrub found errors | O último scrub de uma pool encontrou dados danificados (`dumbmonit_truenas_pool_last_scrub_errors`). | > 0 | 5 min | Warning (`critical`) | 24 h |
| NAS scrub overdue | Uma pool não completou um scrub há mais de 45 dias (`dumbmonit_truenas_pool_last_scrub_age_seconds`). Ausente depois de um resilver, que apaga o registo do último scrub. | > 45 d | 1 h | Info (`info`) | 7 d |
| NAS disk failed its SMART test | Um autoteste SMART no registo de um disco falhou (`dumbmonit_truenas_disk_smart_failed`). Um disco nunca testado não tem série. | > 0 | 5 min | Warning (`critical`) | 24 h |
| NAS disk too hot | Um disco está acima de 55 °C (`dumbmonit_truenas_disk_temperature_celsius`, lido da cache do TrueNAS). Limpa-se aos 50 °C. | > 55 °C | 15 min | Advisory (`warning`) | 6 h |
| NAS dataset near its quota | Um dataset usou mais de 90% da sua quota (`dumbmonit_truenas_dataset_quota_used_percent`): a 100% as suas escritas falham enquanto a pool ainda tem espaço. Sem quota, sem série. | > 90 % | 30 min | Advisory (`warning`) | 24 h |
| NAS replication failed | Uma tarefa de replicação ativada está em erro (`dumbmonit_truenas_replication_error`): a cópia do outro lado está a envelhecer. | > 0 | 10 min | Advisory (`warning`) | 12 h |
| NAS snapshot task failed | Uma tarefa de snapshot periódico ativada está em erro (`dumbmonit_truenas_snapshot_task_error`). | > 0 | 10 min | Advisory (`warning`) | 12 h |
| NAS snapshots stale | Uma tarefa de snapshot periódico ativada não corre há mais de oito dias (`dumbmonit_truenas_snapshot_task_last_run_age_seconds`): uma tarefa que deixa de correr não falha, fica silenciosa. | > 8 d | 1 h | Advisory (`warning`) | 24 h |
| TrueNAS alert raised | O próprio TrueNAS levantou um alerta de nível ERROR ou acima (`dumbmonit_truenas_alerts`). A sua própria frase está na página do monitor. | > 0 | 10 min | Advisory (`warning`) | 12 h |
| NAS service stopped | Um serviço definido para iniciar com o NAS está parado (`dumbmonit_truenas_service_running`). Um serviço desligado de propósito não tem série. | < 1 | 10 min | Advisory (`warning`) | 6 h |
| NAS certificate expiring | O certificado que serve a interface web do TrueNAS expira em menos de 14 dias: os navegadores, as aplicações e os pares de replicação vão recusá-lo (`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`). | < 14 d | 1 h | Advisory (`warning`) | 24 h |
| NAS certificate expired | O certificado que serve a interface web do TrueNAS expira em menos de 2 dias, ou já expirou (`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`). | < 2 d | 15 min | Warning (`critical`) | 12 h |
| NAS boot pool degraded | O pool de arranque já não está saudável: o NAS ainda arranca a partir do dispositivo que resta, até esse também falhar (`dumbmonit_truenas_boot_pool_healthy`). | < 1 | 10 min | Advisory (`warning`) | 12 h |
| NAS boot pool almost full | O pool de arranque está mais de 85 % cheio: os ambientes de arranque antigos acumulam-se a cada atualização e bloqueiam a seguinte. Resolve abaixo de 80 % (`dumbmonit_truenas_boot_pool_used_percent`). | > 85 % | 1 h | Advisory (`warning`) | 24 h |
| NAS remote backup task failed | A última execução de uma tarefa de sincronização na nuvem, de rsync ou de cópia de segurança na nuvem falhou: a cópia remota está a envelhecer (`dumbmonit_truenas_backup_task_failed`). | > 0 | 10 min | Advisory (`warning`) | 12 h |
| NAS app crashed | Uma aplicação está no estado de falha (crashed) (`dumbmonit_truenas_app_crashed`). | > 0 | 10 min | Advisory (`warning`) | 6 h |
| NAS virtual machine stopped | Uma máquina virtual definida para arrancar com o NAS não está em execução (`dumbmonit_truenas_vm_running`). | < 1 | 10 min | Advisory (`warning`) | 6 h |

## Limites conhecidos

- Pela API REST que o DumbMonit utiliza, o TrueNAS não tem chave só de leitura: a chave pertence a um administrador completo. O DumbMonit apenas lê, e nunca inicia um scrub, um teste SMART nem uma replicação.
- A idade do snapshot mais recente de um dataset é a última execução da tarefa de snapshots que o cobre. Um dataset que nenhuma tarefa cobre não tem idade.
- As temperaturas vêm da cache do TrueNAS, atualizada cerca de cinco em cinco minutos. Um disco em repouso nunca é acordado.
- Os resultados dos testes SMART saíram da API no TrueNAS 25.10 e são ignorados a partir dessa versão.
- As regras de certificado só vigiam o certificado que serve a interface web.

## Erros comuns {#common-errors}

| Sintoma | Causa provável |
|---|---|
| Erro de autenticação mencionando Local Administrator | O utilizador da chave não é um administrador completo — tipicamente um Read-Only Administrator. O TrueNAS recusa essa chave em cada chamada REST, com um `403` vazio. |
| Erro de autenticação «Invalid API key» | A chave foi mal digitada, eliminada, expirou — ou revogada pelo TrueNAS porque foi enviada uma vez via HTTP simples de outra máquina. Crie uma nova e mantenha HTTPS. |
| Erro de certificado | Certificado autoassinado: instale um de confiança (ACME está incorporado) ou ative `insecure_tls`. |
| Nenhuma temperatura de disco | O NAS corre numa máquina virtual, ou o TrueNAS ainda não consultou os discos desde que arrancou. As leituras vêm da sua cache. |
| Nenhuma série SMART | Nenhum autoteste correu alguma vez nesse disco, ou o NAS corre o TrueNAS 25.10, que removeu essa chamada. Agende testes SMART em Data Protection. |
| Nenhum `pool_last_scrub_age_seconds` | A última verificação desse pool foi um resilver, que apaga o registo do último scrub, ou o pool nunca foi verificado. |
| Um dataset não tem série de quota | Não tem quota. Uma quota não definida não é uma quota de zero. |
| «NAS pool degraded» e a página não mostra nenhum nome de disco | O pool não pode ser aberto de todo (exportado, ou demasiados discos em falta): o TrueNAS então não reporta nenhuma disposição. |
| `scrape_errors` acima de zero | Uma chamada opcional falhou por um motivo que não um caminho em falta — um tempo limite, geralmente. As outras métricas não são afetadas. |
