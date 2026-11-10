# Unraid

Un servidor Unraid a través de su API GraphQL: un array que no está
iniciado, discos desactivados, ausentes o reportando errores de lectura,
comprobaciones de paridad que encontraron errores o no se han ejecutado
en semanas, pools de caché que se llenan, contenedores que deberían estar
en ejecución y no lo están, y el estado de las VMs.

DumbMonit envía una clave de API con el rol Viewer en la cabecera
`x-api-key` a `/graphql`. El rol Viewer lee todo y no puede cambiar nada.
La API viene integrada en Unraid desde la 7.2; en la 6.12 a 7.1 viene con
el plugin Unraid Connect.

Cada minuto DumbMonit envía una consulta obligatoria (versión, array,
paridad, discos de datos y caché) y cuatro opcionales, cada una por su
cuenta para que un campo que una versión más antigua de la API no tenga,
o las VMs apagadas, solo omitan esa parte: la comprobación de paridad, los
contenedores Docker, las VMs y las notificaciones no leídas.

!!! warning "Validado solo contra la documentación del fabricante — aún no probado en un sistema real"

    Esta integración se construyó a partir de la documentación de la API
    de Unraid y el esquema GraphQL publicado de la API de Unraid
    (nombres de campo, enumeraciones, tamaños en kilobytes), no contra un
    servidor en ejecución. Cada consulta es de lectura. Dinos qué falla.

## Qué vigila {#what-it-watches}

Todas las métricas tienen el prefijo `dumbmonit_unraid_`.

| Métrica | Etiquetas | Significado |
|---|---|---|
| `version_info` | `version` | Valor 1: la versión de Unraid. |
| `array_started` | | 1 cuando el array está iniciado. |
| `array_state_info` | `state` | Valor 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Capacidad del array. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1 cuando el estado del disco es `DISK_OK`. Las ranuras vacías se omiten. |
| `disk_status_info` | `disk`, `role`, `status` | Valor 1: el estado de Unraid (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Errores de lectura desde que arrancó el array (la columna Errors). |
| `disk_temperature_celsius` | `disk`, `role` | Solo mientras el disco gira: Unraid no reporta ninguna para un disco dormido. |
| `disk_spinning` | `disk`, `role` | 1 cuando el disco gira. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Tamaño, tamaño del sistema de archivos y llenado, para discos con un sistema de archivos. |
| `disks`, `disks_problem`, `disks_with_errors` | | Recuentos. |
| `parity_check_status_info` | `status` | Valor 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Una comprobación en curso. |
| `parity_check_ok` | | Tras una comprobación terminada: 1 sin errores de sincronización, 0 con errores o si falló. Una comprobación cancelada no demuestra nada y no fija nada. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | La última comprobación. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Contenedores Docker. |
| `container_running`, `container_autostart_stopped` | `container` | Por contenedor; el segundo es 1 cuando un contenedor configurado para iniciarse con el array no está en ejecución. |
| `vms`, `vms_running` | | VMs. |
| `vm_state_info`, `vm_crashed` | `vm` (y `state`) | Por VM. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Notificaciones no leídas. |
| `registration_ok` | | 1 cuando Unraid no indica ningún problema de licencia, 0 cuando el estado de la licencia es un error (prueba caducada, clave que no corresponde a la memoria USB, archivo de clave ausente…). |
| `registration_info` | `state`, `type` | Valor 1: el estado de la licencia tal como lo indica Unraid (`trial`, `pro`, `eexpired`, `eguid`…) y el tipo de licencia. |
| `trial_expiry_seconds` | | Hasta que termine la licencia de prueba; solo durante una prueba. |
| `os_updates_expiry_seconds` | | Hasta que la licencia deje de tener derecho a las actualizaciones de Unraid OS; ausente cuando la licencia no tiene esa fecha. |
| `uptime_seconds` | | Desde que arrancó el servidor. |
| `cpu_usage_percent` | | Carga del procesador, todos los núcleos juntos. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Memoria. Cuenta como usada la que no está disponible: la caché que Unraid puede devolver no se cuenta. |
| `swap_used_percent` | | Solo cuando el servidor tiene swap. |

La licencia y la carga del sistema (tiempo en marcha, procesador, memoria) proceden de dos consultas opcionales más. Una versión anterior de la API que no las conoce solo deja fuera esas filas: el resto del monitor no se ve afectado.

Las [reglas integradas](../alerting/rules.md#unraid) que se aplican:

| Regla | Qué | Umbral por defecto | Espera | Severidad | Recordatorio |
|---|---|---|---|---|---|
| Unraid array stopped | El array de Unraid no está iniciado: las comparticiones, contenedores y VMs que viven en él no están disponibles (`dumbmonit_unraid_array_started`). | < 1 | 10 min | Warning (`critical`) | 6 h |
| Unraid disk disabled or missing | Un disco de Unraid está desactivado, ausente o inválido: el array funciona degradado, y un fallo más pierde datos (`dumbmonit_unraid_disk_ok`). | < 1 | 5 min | Warning (`critical`) | 6 h |
| Unraid disk read errors | Un disco de Unraid ha reportado errores de lectura desde que arrancó el array: puede desactivarse en el próximo (`dumbmonit_unraid_disk_errors`). | > 0 | 5 min | Advisory (`warning`) | 1 d |
| Unraid disk too hot | Un disco de array o paridad de Unraid se ha mantenido por encima de 55 °C durante quince minutos (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 min | Advisory (`warning`) | 6 h |
| Unraid parity check found errors | La última comprobación de paridad de Unraid falló o encontró errores de sincronización: la paridad ya no coincide con los datos (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 min | Advisory (`warning`) | 1 d |
| Unraid parity check overdue | La última comprobación de paridad de Unraid tiene más de 40 días: se perdió una comprobación mensual (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 d | 1 h | Advisory (`warning`) | 1 d |
| Unraid cache nearly full | Un pool de caché de Unraid está más del 90 % lleno: las escrituras caen al array lento, o fallan (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 min | Advisory (`warning`) | 1 d |
| Unraid container stopped | Un contenedor Docker configurado para iniciarse con el array no está en ejecución (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 min | Advisory (`warning`) | 6 h |
| Unraid VM crashed | Unraid indica que una máquina virtual ha fallado (crashed) (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| Unraid alert unread | Unraid ha emitido una notificación de la máxima importancia (alert) que nadie ha leído: disco averiado, sobrecalentamiento, error del array… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 min | Advisory (`warning`) | 24 h |
| Unraid array nearly full | El array está lleno en más de un 95 %: las escrituras en los recursos compartidos fallarán pronto. Se resuelve por debajo del 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 h | Advisory (`warning`) | 24 h |
| Unraid memory nearly exhausted | Hace quince minutos que se usa más del 95 % de la memoria: Unraid funciona desde la memoria, y cuando se agota se matan contenedores o VM. Se resuelve por debajo del 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 min | Advisory (`warning`) | 6 h |
| Unraid license problem | Unraid indica un problema de licencia (prueba caducada, clave que no corresponde a la memoria USB, archivo de clave ausente…): puede que el array se niegue a arrancar tras la próxima parada (`dumbmonit_unraid_registration_ok`). | < 1 | 30 min | Advisory (`warning`) | 24 h |
| Unraid trial ending | La licencia de prueba termina en menos de tres días: después, el array ya no arranca (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 d | 1 h | Advisory (`warning`) | 24 h |
| Unraid OS updates eligibility ending | La licencia deja de tener derecho a las actualizaciones de Unraid OS en menos de 30 días. El servidor sigue funcionando; solo deja de recibir versiones nuevas (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 d | 1 h | Info (`info`) | 7 d |

## La página del monitor {#the-device-page}

El panel Array indica en una frase qué va mal, muestra el llenado del
array, la última comprobación de paridad, los contenedores y VMs en
ejecución, y luego lista cada disco — paridad primero, luego datos, luego
caché — con su estado, temperatura y llenado, y los contenedores que
deberían estar en ejecución.

## Crear una clave de API de solo lectura para DumbMonit {#create-a-read-only-api-key-for-dumbmonit}

1. En Unraid 7.2 o posterior la API viene integrada. En Unraid 6.12 a
   7.1, instala primero el plugin Unraid Connect desde la pestaña Apps:
   trae la misma API, y no se necesita ningún inicio de sesión en Unraid
   Connect para uso local.

2. Crea una clave de API con el rol Viewer, que lee todo y no puede
   cambiar nada: en Settings > Management Access > API Keys, o desde la
   terminal de Unraid.

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. Copia la clave. En DumbMonit, introduce la dirección del servidor
   Unraid, por ejemplo «tower.lan», y pega la clave como API key. Si la
   interfaz web se sirve por HTTPS, elige https como protocolo.

!!! warning
    Unraid no reporta ninguna temperatura para un disco que está
    detenido, y DumbMonit nunca despierta uno: un disco dormido
    simplemente no tiene temperatura hasta que empieza a girar.

## Credenciales {#credentials}

| Credencial | Campos |
|---|---|
| Clave de API | Una clave con el rol Viewer, enviada como `x-api-key`. |

Dirección: el nombre de host o IP del servidor Unraid, `host:puerto`, o
una URL. Opciones:

- **Protocol** (`http`) y **Port** (80): los de la interfaz web. Con SSL
  definido como Strict en Settings > Management Access, usa `https`, y
  la dirección `myunraid.net` si ese es el certificado.
- **Accept an unverifiable certificate**: para un certificado
  autofirmado.
- **Timeout per request** (15 s).

## Límites conocidos

- El SAI no se lee: conéctelo mejor a través de [NUT](nut.md).
- Sin detalle SMART: DumbMonit lee el estado, los errores y la temperatura que Unraid da para cada disco, no los atributos SMART.
- No se lee si un contenedor tiene una actualización de imagen pendiente.
- Las cifras de licencia, procesador y memoria necesitan una versión reciente de la API de Unraid. En una versión que no las tiene, esas filas y sus reglas simplemente faltan.

## Solución de problemas {#troubleshooting}

«Unraid refused the API key (401)»: la clave es incorrecta o fue
eliminada.

«Unraid accepted the API key but refused to answer (403)»: la clave no
tiene ningún rol, o un rol más limitado que Viewer.

Sin cifras de contenedor, VM o paridad: esa consulta opcional falló (VMs
apagadas, una versión más antigua de la API sin el campo); el resto se
sigue leyendo. El motivo está en el registro del servidor a nivel debug.
