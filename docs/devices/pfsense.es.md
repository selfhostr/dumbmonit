# pfSense

Un firewall pfSense a través del paquete comunitario de API REST
(`pfSense-pkg-RESTAPI`, versión 2): gateways caídos, que pierden paquetes
o responden lento, interfaces activadas sin enlace, servicios activados
que se detuvieron, y CPU, memoria, disco y temperatura.

DumbMonit envía una clave de API en la cabecera `X-API-Key`. Una clave
lleva exactamente los privilegios del usuario pfSense al que pertenece:
el usuario `dumbmonit` de abajo solo tiene los privilegios `GET` de las
cinco páginas que lee DumbMonit, así que la clave puede leer esas páginas
y no cambiar nada. Cada minuto DumbMonit lee, en paralelo,
`/api/v2/status/system`, `/api/v2/status/gateways`,
`/api/v2/status/interfaces`, `/api/v2/status/services` y
`/api/v2/system/version`, y, si se dio el privilegio,
`/api/v2/system/restapi/version`.

!!! warning "Validado solo contra la documentación del fabricante — aún no probado en un sistema real"

    Esta integración se construyó a partir de la documentación publicada
    del paquete de API REST y el código fuente de sus modelos (nombres
    de campo, unidades, el sobre de respuesta y sus identificadores de
    error), no contra un pfSense en ejecución. Cada llamada es de
    lectura. Dinos qué falla.

## Qué vigila {#what-it-watches}

Todas las métricas tienen el prefijo `dumbmonit_pfsense_`.

| Métrica | Etiquetas | Significado |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Como las muestra el panel, en porcentaje. La cifra de CPU es la propia estimación de pfSense a partir del load average. |
| `temperature_celsius` | | Cuando pfSense puede leer un sensor. |
| `load_average` | `period`: `1m`, `5m`, `15m` | Load average. |
| `cpu_count` | | Núcleos del procesador. |
| `version_info` | `version` | Valor 1: la versión de pfSense, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0 cuando pfSense marca el gateway como caído, 1 en caso contrario. |
| `gateway_status_info` | `gateway`, `status` | Valor 1: la propia palabra de pfSense (`none` cuando todo va bien, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Pérdida de paquetes hacia la dirección de monitor. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Ida y vuelta hacia la dirección de monitor y su desviación; ausente mientras el gateway está caído. |
| `gateways`, `gateways_down` | | Recuentos. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | Activada en la configuración; enlace levantado. |
| `interface_down` | `interface`, `descr` | 1 cuando la interfaz está activada y no tiene enlace. Una interfaz desactivada nunca está caída. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Contadores. |
| `interfaces_down` | | Recuento. |
| `service_running`, `service_enabled` | `service`, `description` | Como lo muestra Status > Services. |
| `service_stopped` | `service`, `description` | 1 cuando el servicio está activado y no en ejecución. |
| `services_stopped` | | Recuento. |
| `states`, `states_max`, `states_used_percent` | | Conexiones que sigue pf, el límite y la parte en uso. Necesita el privilegio `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1 cuando CARP está activado; 1 cuando se ha puesto en modo de mantenimiento persistente. Necesita el privilegio `/api/v2/status/carp GET`. |
| `restapi_update_available` | | Con el privilegio `/api/v2/system/restapi/version`: 1 cuando el paquete de API REST tiene una versión más reciente. |
| `restapi_version_info` | `version`, `latest` | Valor 1: versión instalada y última versión del paquete. |

pfSense no indica a través de esta API si hay una actualización de
pfSense disponible: la única ruta que toca las actualizaciones del
sistema (`/api/v2/system/update`) inicia una. DumbMonit solo reporta las
propias actualizaciones del paquete.

La tabla de estados y CARP son lecturas opcionales. Cada una necesita un privilegio más en el usuario `dumbmonit`: `/api/v2/firewall/states/size GET` y `/api/v2/status/carp GET`. Sin el privilegio, pfSense rechaza esa lectura y las cifras correspondientes simplemente faltan: ni error, ni alerta.

Las [reglas integradas](../alerting/rules.md#pfsense) que se aplican:

| Regla | Qué | Umbral por defecto | Espera | Severidad | Recordatorio |
|---|---|---|---|---|---|
| pfSense gateway down | Un gateway pfSense está caído: su dirección de monitor ya no responde, y el tráfico que lleva se detiene o se conmuta (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 min | Warning (`critical`) | 6 h |
| pfSense gateway losing packets | Un gateway pfSense pierde más del 10 % de las sondas hacia su dirección de monitor: el enlace está degradado (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense gateway slow | La ida y vuelta hacia la dirección de monitor de un gateway pfSense se ha mantenido por encima de 500 ms durante quince minutos (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 ms | 15 min | Advisory (`warning`) | 6 h |
| pfSense interface without link | Una interfaz pfSense activada no tiene enlace: cable, puerto de switch o tarjeta de red (`dumbmonit_pfsense_interface_down`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| pfSense service stopped | Un servicio pfSense que está activado no está en ejecución: resolutor DNS, VPN, NTP… (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| pfSense disk nearly full | El disco de pfSense está más del 90 % lleno: los logs, informes y actualizaciones de paquete dejan de escribirse (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 min | Advisory (`warning`) | 1 d |
| pfSense state table nearly full | pf sigue más del 80 % de las conexiones que tiene permitidas: pasado el límite, el cortafuegos descarta las conexiones nuevas sin ningún otro síntoma. Se resuelve por debajo del 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense network buffers nearly exhausted | Más del 90 % de los búferes de red están en uso: un cortafuegos que se queda sin ellos deja de reenviar tráfico aunque su procesador esté inactivo. Se resuelve por debajo del 80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense temperature high | La temperatura que lee pfSense se ha mantenido por encima de 85 °C durante diez minutos: el equipo está a punto de ralentizarse a sí mismo. Se resuelve a 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 min | Advisory (`warning`) | 6 h |
| pfSense CPU high | El procesador se ha mantenido por encima del 90 % durante quince minutos. Se resuelve por debajo del 80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 min | Advisory (`warning`) | 6 h |
| pfSense swap in use | pfSense lleva una hora usando más de la mitad de su swap: le falta memoria (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 h | Advisory (`warning`) | 24 h |
| pfSense interface errors | Una interfaz ha registrado más de 100 errores de recepción o de transmisión en quince minutos: revise el cable, el módulo SFP y el ajuste de dúplex (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 min | Advisory (`warning`) | 24 h |
| pfSense CARP maintenance mode left on | CARP lleva una hora en modo de mantenimiento persistente: este cortafuegos cedió sus direcciones virtuales a su compañero y no las recuperará por sí solo (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 h | Info (`info`) | 24 h |

## La página del monitor {#the-device-page}

El panel Firewall indica en una frase qué va mal, muestra CPU, memoria,
disco y temperatura, luego lista cada gateway con su latencia y pérdida,
las interfaces sin enlace, los servicios detenidos, y una actualización
disponible del paquete de API REST.

## Crear una clave de API REST de solo lectura para DumbMonit {#create-a-read-only-rest-api-key-for-dumbmonit}

1. Instala el paquete comunitario de API REST (pfSense-pkg-RESTAPI,
   versión 2) desde Diagnostics > Command Prompt o una shell SSH, con el
   archivo de la página de versiones del proyecto que coincida con tu
   versión de pfSense (2.7.2 abajo). Luego, en System > REST API >
   Settings, marca tanto BasicAuth como Key entre los métodos de
   autenticación.

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. En System > User Manager, añade un usuario llamado dumbmonit con una
   contraseña larga y aleatoria. En Effective Privileges, añade solo
   estos privilegios de API REST: /api/v2/status/system GET,
   /api/v2/status/gateways GET, /api/v2/status/interfaces GET,
   /api/v2/status/services GET, /api/v2/system/version GET, más
   /api/v2/system/restapi/version GET para enterarte de las propias
   actualizaciones del paquete. Solo para el siguiente paso, añade
   también /api/v2/auth/key POST.

3. Crea la clave como dumbmonit, desde cualquier máquina que alcance
   pfSense: la clave lleva exactamente los privilegios de ese usuario.
   Copia el valor de clave de la respuesta, y luego retira de nuevo el
   privilegio /api/v2/auth/key POST.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. En DumbMonit, introduce la dirección de pfSense, por ejemplo
   «pfsense.lan», y pega la clave como API key. pfSense viene con un
   certificado autofirmado: marca Accept an unverifiable certificate a
   menos que lo hayas reemplazado.

!!! warning
    La API REST no indica si hay una actualización de pfSense
    disponible, solo si el propio paquete tiene una. El paquete es un
    proyecto comunitario, no parte de pfSense: reinstálalo después de
    cada actualización de pfSense.

## Credenciales {#credentials}

| Credencial | Campos |
|---|---|
| Clave de API REST | La clave creada para `dumbmonit` en el paso 3, enviada como `X-API-Key`. |

Dirección: el nombre de host o IP de pfSense, `host:puerto`, o una URL.
Opciones:

- **Protocol** (`https`) y **Port** (443): los del webConfigurator,
  definidos en System > Advanced > Admin Access.
- **Accept an unverifiable certificate**: el propio certificado de
  pfSense es autofirmado.
- **Timeout per request** (15 s).

## Límites conocidos

- Los certificados no se leen: el paquete devuelve sus claves privadas junto con ellos, y DumbMonit se niega a pedirlos. Vigile una fecha de caducidad con un [monitor TLS](services.md#tls) sobre la dirección que sirve el certificado.
- Los túneles VPN (IPsec, OpenVPN, WireGuard) no se siguen uno a uno. Sus servicios sí, mediante «pfSense service stopped».
- Las actualizaciones de pfSense no se indican: la única ruta de la API que las toca inicia una.
- La memoria no tiene regla: pfSense cuenta la caché de ZFS como memoria usada, así que una cifra alta es normal. El swap en uso es la señal fiable de un cortafuegos al que le falta memoria.

## Solución de problemas {#troubleshooting}

«pfSense refused the API key (401)»: la clave es incorrecta, o Key no
está marcado entre los métodos de autenticación en System > REST API >
Settings.

«pfSense accepted the API key but refused … (403)»: al usuario
`dumbmonit` le falta el privilegio de esa página; el mensaje lo nombra.

«… not found: is the REST API package installed?»: falta el paquete,
normalmente tras una actualización de pfSense, que lo elimina.
Reinstálalo como en el paso 1.
