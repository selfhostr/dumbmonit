//! Fusion du fichier et de l'environnement en une `Config` validée.

use super::*;

impl Config {
    pub(super) fn merge_at(
        file: FileConfig,
        env: EnvSource,
        config_path: Option<&Path>,
    ) -> Result<Self> {
        // `server_urls` : la liste ordonnée des serveurs. Sans `server_url`, la
        // première adresse de la liste est la primaire.
        let mut listed: Vec<ServerEndpoint> = match env.get("DUMBMONIT_AGENT_URLS") {
            Some(raw) => split_list(&raw)
                .into_iter()
                .map(|url| ServerEndpoint { url, tls_url: None })
                .collect(),
            None => file.server_urls.map(ServerListFile::into_endpoints).unwrap_or_default(),
        };
        for endpoint in &mut listed {
            endpoint.url = clean_url(&endpoint.url);
            endpoint.tls_url =
                endpoint.tls_url.as_deref().map(clean_url).filter(|url| !url.is_empty());
        }
        listed.retain(|endpoint| !endpoint.url.is_empty());
        let explicit = env
            .get("DUMBMONIT_AGENT_URL")
            .or(file.server_url)
            .map(|url| clean_url(&url))
            .filter(|url| !url.is_empty());
        let (server_url, listed_tls_url) = match explicit {
            Some(url) => (url, None),
            None if !listed.is_empty() => {
                let first = listed.remove(0);
                (first.url, first.tls_url)
            }
            None => (String::new(), None),
        };
        if server_url.is_empty() {
            bail!(
                "the server URL is required (key 'server_url' or 'server_urls', or variable \
                 DUMBMONIT_AGENT_URL)"
            );
        }
        if !server_url.starts_with("http://") && !server_url.starts_with("https://") {
            bail!("the server URL must start with https:// (got '{server_url}')");
        }
        let mut fallback_servers: Vec<ServerEndpoint> = Vec::new();
        for endpoint in listed {
            if endpoint.url == server_url
                || fallback_servers.iter().any(|known| known.url == endpoint.url)
            {
                continue;
            }
            // Jamais de secours en clair hors boucle locale : la primaire en
            // clair n'est tolérée que pour un agent déjà enrôlé, une adresse
            // de secours ne sert que de route vers le même serveur.
            let url = &endpoint.url;
            if !url.starts_with("https://") && !is_loopback_http(url) {
                bail!("server_urls: each address must start with https:// (got '{url}')");
            }
            if let Some(url) = &endpoint.tls_url
                && !url.starts_with("https://")
            {
                bail!("server_urls: the agent channel URL must start with https:// (got '{url}')");
            }
            fallback_servers.push(endpoint);
        }
        // Une URL en clair hors boucle locale n'est tolérée que pour un agent
        // déjà enrôlé, qui ne parlera alors qu'à son canal chiffré rangé : c'est
        // `Agent::new` qui le vérifie (il lit l'état du canal), et refuse sinon.
        let plain_server_url =
            !server_url.starts_with("https://") && !is_loopback_http(&server_url);

        let token = env.get("DUMBMONIT_AGENT_TOKEN").or(file.token).unwrap_or_default();
        let token = token.trim().to_string();
        if token.is_empty() {
            bail!(
                "the enrollment token is required (key 'token' or variable DUMBMONIT_AGENT_TOKEN)"
            );
        }

        let tls_url = env
            .get("DUMBMONIT_AGENT_TLS_URL")
            .or(file.tls_url)
            .map(|url| clean_url(&url))
            .filter(|url| !url.is_empty())
            .or(listed_tls_url);
        if let Some(url) = &tls_url
            && !url.starts_with("https://")
        {
            bail!("the agent channel URL must start with https:// (got '{url}')");
        }
        let ca_pin = match env.get("DUMBMONIT_AGENT_CA_PIN").or(file.ca_pin) {
            Some(raw) if !raw.trim().is_empty() => {
                Some(dumbmonit_proto::normalise_pin(&raw).with_context(|| {
                    format!(
                        "ca_pin: a SHA-256 fingerprint (64 hex digits) is expected, got '{raw}'"
                    )
                })?)
            }
            _ => None,
        };

        let server_pin = match env.get("DUMBMONIT_AGENT_SERVER_PIN").or(file.server_pin) {
            Some(raw) if !raw.trim().is_empty() => {
                let pin = raw.trim().to_string();
                if !crate::tls::is_key_pin(&pin) {
                    bail!(
                        "server_pin: a public key pin (sha256//<base64>) is expected, got '{raw}'"
                    );
                }
                Some(pin)
            }
            _ => None,
        };

        let transport = match env
            .get("DUMBMONIT_AGENT_TRANSPORT")
            .filter(|raw| !raw.trim().is_empty())
            .or(file.transport)
        {
            Some(raw) => crate::ws::TransportMode::parse(&raw).with_context(|| {
                format!("transport: auto, direct or websocket is expected, got '{raw}'")
            })?,
            None => crate::ws::TransportMode::Auto,
        };

        let interval_secs = match env.get("DUMBMONIT_AGENT_INTERVAL_SECS") {
            Some(raw) => raw
                .trim()
                .parse::<u64>()
                .with_context(|| format!("DUMBMONIT_AGENT_INTERVAL_SECS: invalid value '{raw}'"))?,
            None => file.interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS),
        };
        if interval_secs < MIN_INTERVAL_SECS {
            bail!("the sampling period must be at least {MIN_INTERVAL_SECS} seconds");
        }

        let services = match env.get("DUMBMONIT_AGENT_SERVICES") {
            Some(raw) => split_list(&raw),
            None => file.services.unwrap_or_default(),
        };

        let tags = match env.get("DUMBMONIT_AGENT_TAGS") {
            Some(raw) => parse_tags(&raw)?,
            None => file.tags.unwrap_or_default(),
        };

        let flag = |variable: &str, from_file: Option<bool>, default: bool| -> Result<bool> {
            match env.get(variable) {
                Some(raw) => parse_bool(&raw).with_context(|| format!("{variable}: {raw}")),
                None => Ok(from_file.unwrap_or(default)),
            }
        };
        // Sans liste, chaque module se détecte seul (comportement historique).
        // Avec une liste, seuls ceux qu'elle nomme tournent : un drapeau
        // explicite (`smart: true`, `DUMBMONIT_AGENT_SMART=1`) reste maître.
        let modules = parse_modules(
            env.get("DUMBMONIT_AGENT_MODULES").map(|raw| split_list(&raw)),
            file.modules.map(Patterns::into_items),
        )?;
        let wanted = |module: &str| modules.as_ref().is_none_or(|list| list.contains(module));
        let docker = flag("DUMBMONIT_AGENT_DOCKER", file.docker, wanted("docker"))?;
        let docker_update_check =
            flag("DUMBMONIT_AGENT_DOCKER_UPDATE_CHECK", file.docker_update_check, true)?;
        let commands = flag("DUMBMONIT_AGENT_COMMANDS", file.commands, true)?;
        let relay = flag("DUMBMONIT_AGENT_RELAY", file.relay, false)?;
        // Remédiation : refusée tant que la machine ne l'a pas demandée.
        let actions = crate::remediation::ActionsConfig {
            enabled: flag("DUMBMONIT_AGENT_ACTIONS", file.actions, false)?,
            dir: env
                .get("DUMBMONIT_AGENT_ACTIONS_DIR")
                .or(file.actions_dir)
                .map(|dir| dir.trim().to_string())
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::remediation::default_dir(config_path)),
            services: match env.get("DUMBMONIT_AGENT_ACTION_SERVICES") {
                Some(raw) => split_list(&raw),
                None => file.action_services.unwrap_or_default(),
            },
        };
        // Contrôles personnalisés : refusés tant que la machine ne les a pas
        // demandés, et seulement dans les dossiers qu'elle nomme.
        let scripts_dirs: Vec<PathBuf> = match env.get("DUMBMONIT_AGENT_SCRIPTS_DIRS") {
            Some(raw) => split_list(&raw),
            None => file.scripts_dirs.unwrap_or_default(),
        }
        .into_iter()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .collect();
        let scripts = crate::scripts::ScriptsConfig {
            enabled: flag("DUMBMONIT_AGENT_SCRIPTS", file.scripts, false)?,
            dirs: if scripts_dirs.is_empty() {
                crate::scripts::default_dirs(config_path)
            } else {
                scripts_dirs
            },
            user: env
                .get("DUMBMONIT_AGENT_SCRIPTS_USER")
                .or(file.scripts_user)
                .map(|user| user.trim().to_string())
                .filter(|user| !user.is_empty()),
            max_concurrent: file
                .scripts_max_concurrent
                .unwrap_or(crate::scripts::DEFAULT_MAX_CONCURRENT)
                .clamp(1, 16),
        };
        let site = env
            .get("DUMBMONIT_AGENT_SITE")
            .or(file.site)
            .map(|site| site.trim().to_string())
            .filter(|site| !site.is_empty());
        let gateway = gateway_config(
            &env,
            file.gateway_listen,
            file.gateway_max_connections,
            file.gateway_rate_kib,
        )?;
        let cpu_per_core = flag("DUMBMONIT_AGENT_CPU_PER_CORE", file.cpu_per_core, false)?;

        let docker_max_containers = match env.get("DUMBMONIT_AGENT_DOCKER_MAX_CONTAINERS") {
            Some(raw) => raw.trim().parse::<usize>().with_context(|| {
                format!("DUMBMONIT_AGENT_DOCKER_MAX_CONTAINERS: invalid value '{raw}'")
            })?,
            None => file.docker_max_containers.unwrap_or(DEFAULT_MAX_CONTAINERS),
        };

        // Trois listes de motifs, même règle : l'environnement remplace le
        // fichier, le fichier remplace la valeur par défaut — jamais de fusion,
        // qui empêcherait de retirer un motif par défaut.
        let patterns =
            |variable: &str, key: &str, from_file: Option<Patterns>, default: &[&str]| {
                let items = match env.get(variable) {
                    Some(raw) => split_list(&raw),
                    None => match from_file {
                        Some(patterns) => patterns.into_items(),
                        None => default.iter().map(|d| d.to_string()).collect(),
                    },
                };
                NameFilter::parse(key, &items)
            };
        let probe = ProbeConfig {
            interfaces_ignore: patterns(
                "DUMBMONIT_AGENT_INTERFACES_IGNORE",
                "interfaces_ignore",
                file.interfaces_ignore,
                &[DEFAULT_INTERFACES_IGNORE],
            )?,
            interfaces_only: patterns(
                "DUMBMONIT_AGENT_INTERFACES_ONLY",
                "interfaces_only",
                file.interfaces_only,
                &[],
            )?,
            mounts_ignore: patterns(
                "DUMBMONIT_AGENT_MOUNTS_IGNORE",
                "mounts_ignore",
                file.mounts_ignore,
                &[DEFAULT_MOUNTS_IGNORE],
            )?,
            cpu_per_core,
        };

        let plakar_interval_secs = match env.get("DUMBMONIT_AGENT_PLAKAR_INTERVAL_SECS") {
            Some(raw) => raw.trim().parse::<u64>().with_context(|| {
                format!("DUMBMONIT_AGENT_PLAKAR_INTERVAL_SECS: invalid value '{raw}'")
            })?,
            None => file.plakar_interval_secs.unwrap_or(plakar::DEFAULT_INTERVAL_SECS),
        };
        if plakar_interval_secs < plakar::MIN_INTERVAL_SECS {
            bail!(
                "the Plakar reading period must be at least {} seconds",
                plakar::MIN_INTERVAL_SECS
            );
        }
        let plakar = PlakarConfig {
            enabled: flag("DUMBMONIT_AGENT_PLAKAR", file.plakar, wanted("plakar"))?,
            bin: env
                .get("DUMBMONIT_AGENT_PLAKAR_BIN")
                .or(file.plakar_bin)
                .map(|bin| bin.trim().to_string())
                .filter(|bin| !bin.is_empty())
                .unwrap_or_else(|| "plakar".to_string()),
            klosets: match env.get("DUMBMONIT_AGENT_PLAKAR_KLOSETS") {
                Some(raw) => split_list(&raw),
                None => file
                    .plakar_klosets
                    .unwrap_or_default()
                    .into_iter()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .collect(),
            },
            home: env
                .get("DUMBMONIT_AGENT_PLAKAR_HOME")
                .or(file.plakar_home)
                .map(|home| home.trim().to_string())
                .filter(|home| !home.is_empty()),
            interval: Duration::from_secs(plakar_interval_secs),
        };

        // Matériel : sondes, disques, pools ZFS. Chacun se détecte tout seul et
        // se tait quand il ne trouve rien ; le drapeau ne sert qu'à couper la
        // détection elle-même.
        let sensors = flag("DUMBMONIT_AGENT_SENSORS", file.sensors, wanted("sensors"))?;
        let smart = SmartConfig {
            enabled: flag("DUMBMONIT_AGENT_SMART", file.smart, wanted("smart"))?,
            bin: command_name(env.get("DUMBMONIT_AGENT_SMART_BIN").or(file.smart_bin), "smartctl"),
            interval: Duration::from_secs(read_interval(
                &env,
                "DUMBMONIT_AGENT_SMART_INTERVAL_SECS",
                file.smart_interval_secs,
                smart::DEFAULT_INTERVAL_SECS,
                smart::MIN_INTERVAL_SECS,
                "the SMART reading period",
            )?),
        };
        let zfs = ZfsConfig {
            enabled: flag("DUMBMONIT_AGENT_ZFS", file.zfs, wanted("zfs"))?,
            bin: command_name(env.get("DUMBMONIT_AGENT_ZFS_BIN").or(file.zfs_bin), "zpool"),
            interval: Duration::from_secs(read_interval(
                &env,
                "DUMBMONIT_AGENT_ZFS_INTERVAL_SECS",
                file.zfs_interval_secs,
                zfs::DEFAULT_INTERVAL_SECS,
                zfs::MIN_INTERVAL_SECS,
                "the ZFS reading period",
            )?),
        };
        let libvirt = LibvirtConfig {
            enabled: flag("DUMBMONIT_AGENT_LIBVIRT", file.libvirt, wanted("libvirt"))?,
            bin: command_name(env.get("DUMBMONIT_AGENT_LIBVIRT_BIN").or(file.libvirt_bin), "virsh"),
            uri: command_name(
                env.get("DUMBMONIT_AGENT_LIBVIRT_URI").or(file.libvirt_uri),
                libvirt::DEFAULT_URI,
            ),
            interval: Duration::from_secs(read_interval(
                &env,
                "DUMBMONIT_AGENT_LIBVIRT_INTERVAL_SECS",
                file.libvirt_interval_secs,
                libvirt::DEFAULT_INTERVAL_SECS,
                libvirt::MIN_INTERVAL_SECS,
                "the libvirt reading period",
            )?),
        };

        let wireguard = WireguardConfig {
            enabled: flag("DUMBMONIT_AGENT_WIREGUARD", file.wireguard, wanted("wireguard"))?,
            bin: command_name(
                env.get("DUMBMONIT_AGENT_WIREGUARD_BIN").or(file.wireguard_bin),
                "wg",
            ),
            peer_names: file
                .wireguard_peer_names
                .unwrap_or_default()
                .into_iter()
                .map(|(key, name)| (key.trim().to_string(), name.trim().to_string()))
                .filter(|(key, name)| !key.is_empty() && !name.is_empty())
                .collect(),
        };
        let ipsec = IpsecConfig {
            enabled: flag("DUMBMONIT_AGENT_IPSEC", file.ipsec, wanted("ipsec"))?,
            bin: command_name(env.get("DUMBMONIT_AGENT_IPSEC_BIN").or(file.ipsec_bin), "swanctl"),
        };

        let speedtest_interval_secs = match env.get("DUMBMONIT_AGENT_SPEEDTEST_INTERVAL_SECS") {
            Some(raw) => raw.trim().parse::<u64>().with_context(|| {
                format!("DUMBMONIT_AGENT_SPEEDTEST_INTERVAL_SECS: invalid value '{raw}'")
            })?,
            None => file.speedtest_interval_secs.unwrap_or(speedtest::DEFAULT_INTERVAL_SECS),
        };
        if speedtest_interval_secs < speedtest::MIN_INTERVAL_SECS {
            bail!("the speedtest period must be at least {} seconds", speedtest::MIN_INTERVAL_SECS);
        }
        let clean_url =
            |raw: Option<String>| raw.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
        let opt_f64 = |variable: &str, from_file: Option<f64>| -> Result<Option<f64>> {
            match env.get(variable) {
                Some(raw) if raw.trim().is_empty() => Ok(None),
                Some(raw) => Ok(Some(
                    raw.trim()
                        .parse::<f64>()
                        .with_context(|| format!("{variable}: invalid value '{raw}'"))?,
                )),
                None => Ok(from_file),
            }
        };
        let inventory = flag("DUMBMONIT_AGENT_INVENTORY", file.inventory, true)?;
        let self_update = flag("DUMBMONIT_AGENT_SELF_UPDATE", file.self_update, true)?;
        let speedtest = SpeedtestConfig {
            enabled: flag("DUMBMONIT_AGENT_SPEEDTEST", file.speedtest, false)?,
            download_url: clean_url(
                env.get("DUMBMONIT_AGENT_SPEEDTEST_DOWNLOAD_URL").or(file.speedtest_download_url),
            ),
            upload_url: clean_url(
                env.get("DUMBMONIT_AGENT_SPEEDTEST_UPLOAD_URL").or(file.speedtest_upload_url),
            ),
            download_bytes_cap: match env.get("DUMBMONIT_AGENT_SPEEDTEST_DOWNLOAD_BYTES_CAP") {
                Some(raw) => raw.trim().parse::<u64>().with_context(|| {
                    format!("DUMBMONIT_AGENT_SPEEDTEST_DOWNLOAD_BYTES_CAP: invalid value '{raw}'")
                })?,
                None => file
                    .speedtest_download_bytes_cap
                    .unwrap_or(speedtest::DEFAULT_DOWNLOAD_BYTES_CAP),
            },
            upload_bytes: match env.get("DUMBMONIT_AGENT_SPEEDTEST_UPLOAD_BYTES") {
                Some(raw) => raw.trim().parse::<u64>().with_context(|| {
                    format!("DUMBMONIT_AGENT_SPEEDTEST_UPLOAD_BYTES: invalid value '{raw}'")
                })?,
                None => file.speedtest_upload_bytes.unwrap_or(speedtest::DEFAULT_UPLOAD_BYTES),
            },
            interval: Duration::from_secs(speedtest_interval_secs),
            min_download_bytes_per_second: opt_f64(
                "DUMBMONIT_AGENT_SPEEDTEST_MIN_DOWNLOAD_MBPS",
                file.speedtest_min_download_mbps,
            )?
            .map(|mbps| mbps * MBPS_IN_BYTES_PER_SECOND),
            min_upload_bytes_per_second: opt_f64(
                "DUMBMONIT_AGENT_SPEEDTEST_MIN_UPLOAD_MBPS",
                file.speedtest_min_upload_mbps,
            )?
            .map(|mbps| mbps * MBPS_IN_BYTES_PER_SECOND),
            max_latency_ms: opt_f64(
                "DUMBMONIT_AGENT_SPEEDTEST_MAX_LATENCY_MS",
                file.speedtest_max_latency_ms,
            )?,
        };
        let backup_repos = BackupReposConfig {
            restic_bin: command_name(
                env.get("DUMBMONIT_AGENT_RESTIC_BIN").or(file.restic_bin),
                "restic",
            ),
            borg_bin: command_name(env.get("DUMBMONIT_AGENT_BORG_BIN").or(file.borg_bin), "borg"),
            repos: backup_repo_list(file.restic_repos, file.borg_repos)?,
            interval: Duration::from_secs(read_interval(
                &env,
                "DUMBMONIT_AGENT_BACKUP_REPOS_INTERVAL_SECS",
                file.backup_repos_interval_secs,
                backup_repos::DEFAULT_INTERVAL_SECS,
                backup_repos::MIN_INTERVAL_SECS,
                "the restic/Borg reading period",
            )?),
        };

        let databases =
            Self::merge_databases(file.databases.unwrap_or_default(), &env, wanted("databases"))?;

        let perf_counters = Self::merge_perf_counters(
            file.perf_counters,
            file.mdaemon,
            file.hyperv,
            &env,
            wanted("perf_counters"),
        )?;

        let max_buffered_samples = match env.get("DUMBMONIT_AGENT_MAX_BUFFERED_SAMPLES") {
            Some(raw) => raw.trim().parse::<usize>().with_context(|| {
                format!("DUMBMONIT_AGENT_MAX_BUFFERED_SAMPLES: invalid value '{raw}'")
            })?,
            None => file.max_buffered_samples.unwrap_or(DEFAULT_MAX_BUFFERED_SAMPLES),
        };
        if max_buffered_samples == 0 {
            bail!("the catch-up buffer cannot be zero");
        }
        let buffer_max_mb = match env.get("DUMBMONIT_AGENT_BUFFER_MAX_MB") {
            Some(raw) => raw
                .trim()
                .parse::<u64>()
                .with_context(|| format!("DUMBMONIT_AGENT_BUFFER_MAX_MB: invalid value '{raw}'"))?,
            None => file.buffer_max_mb.unwrap_or(DEFAULT_BUFFER_MAX_MB),
        };

        let log_level = match env.get("DUMBMONIT_AGENT_LOG").or(file.log_level) {
            Some(raw) => parse_level(&raw)?,
            None => tracing::Level::INFO,
        };

        let system_health = Self::merge_system_health(
            file.system_health.unwrap_or_default(),
            &env,
            wanted("system_health"),
        )?;

        // Mandataire vers le serveur : la clé `proxy` (ou sa variable), à défaut
        // HTTPS_PROXY et consorts, que le client HTTP lit de lui-même.
        // Une variable vide (celles de docker-compose.agent.yml) laisse le fichier
        // décider.
        let set = |name: &str| env.get(name).filter(|value| !value.is_empty());
        let proxy = crate::proxy::ProxyConfig::parse(
            set("DUMBMONIT_AGENT_PROXY").or(file.proxy).as_deref(),
            match set("DUMBMONIT_AGENT_NO_PROXY") {
                Some(raw) => split_list(&raw),
                None => file.no_proxy.map(Patterns::into_items).unwrap_or_default(),
            },
            set("DUMBMONIT_AGENT_PROXY_USER").or(file.proxy_user).as_deref(),
            set("DUMBMONIT_AGENT_PROXY_PASSWORD").or(file.proxy_password).as_deref(),
        )?;

        Ok(Self {
            server_url,
            token,
            tls_url,
            ca_pin,
            server_pin,
            plain_server_url,
            fallback_servers,
            transport,
            interval: Duration::from_secs(interval_secs),
            hostname: env
                .get("DUMBMONIT_AGENT_HOSTNAME")
                .or(file.hostname)
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
            services,
            tags,
            modules,
            docker,
            docker_socket: env
                .get("DUMBMONIT_AGENT_DOCKER_SOCKET")
                .or(file.docker_socket)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_DOCKER_SOCKET)),
            docker_update_check,
            docker_max_containers,
            commands,
            actions,
            scripts,
            relay,
            site,
            gateway,
            probe,
            system_health,
            plakar,
            sensors,
            smart,
            zfs,
            libvirt,
            wireguard,
            ipsec,
            speedtest,
            inventory,
            self_update,
            backup_repos,
            databases,
            perf_counters,
            max_buffered_samples,
            buffer_max_mb,
            secret_path: Self::resolve_secret_path(&env, config_path),
            log_level,
            proxy,
        })
    }

    /// Compteurs de performance : la variable remplace la liste du fichier.
    ///
    /// Dans la variable, les chemins sont séparés par des points-virgules et non
    /// des virgules : une instance comme `\Processor Information(0,1)` en
    /// contient. Chaque chemin est vérifié au démarrage, pour qu'une barre
    /// oubliée se voie tout de suite plutôt qu'en creux dans les graphes.
    pub(super) fn merge_perf_counters(
        from_file: Option<Vec<PerfCounterFile>>,
        mdaemon: Option<FlagOrAuto>,
        hyperv: Option<FlagOrAuto>,
        env: &EnvSource,
        wanted: bool,
    ) -> Result<PerfCountersConfig> {
        let counters: Vec<CustomCounter> = match env.get("DUMBMONIT_AGENT_PERF_COUNTERS") {
            Some(raw) => raw
                .split(';')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(|path| CustomCounter { path: path.to_string(), name: path.to_string() })
                .collect(),
            None => from_file
                .unwrap_or_default()
                .into_iter()
                .map(|entry| match entry {
                    PerfCounterFile::Path(path) => {
                        let path = path.trim().to_string();
                        CustomCounter { name: path.clone(), path }
                    }
                    PerfCounterFile::Named { path, name } => {
                        let path = path.trim().to_string();
                        let name = name
                            .map(|n| n.trim().to_string())
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| path.clone());
                        CustomCounter { path, name }
                    }
                })
                .filter(|counter| !counter.path.is_empty())
                .collect(),
        };
        for counter in &counters {
            perf_counters::parse_path(&counter.path).map_err(|reason| {
                anyhow::anyhow!("perf_counters: {reason} (expected \\Object(Instance)\\Counter)")
            })?;
        }
        if counters.len() > perf_counters::MAX_CUSTOM_COUNTERS {
            bail!(
                "perf_counters: {} counters listed, at most {} are read",
                counters.len(),
                perf_counters::MAX_CUSTOM_COUNTERS
            );
        }
        let mdaemon =
            Self::preset_mode("mdaemon", "DUMBMONIT_AGENT_MDAEMON", mdaemon, env, wanted)?;
        let hyperv = Self::preset_mode("hyperv", "DUMBMONIT_AGENT_HYPERV", hyperv, env, wanted)?;
        Ok(PerfCountersConfig { counters, mdaemon, hyperv })
    }

    /// Un jeu prédéfini de compteurs : `true`, `false` ou `auto` (le défaut —
    /// `false` quand une liste `modules` omet `perf_counters`), la variable
    /// d'environnement primant sur le fichier.
    pub(super) fn preset_mode(
        key: &str,
        variable: &str,
        from_file: Option<FlagOrAuto>,
        env: &EnvSource,
        wanted: bool,
    ) -> Result<PresetMode> {
        Ok(match env.get(variable) {
            Some(raw) => PresetMode::parse(&raw).with_context(|| {
                format!("{variable}: expected true, false or auto, got '{raw}'")
            })?,
            None => match from_file {
                None if wanted => PresetMode::Auto,
                None => PresetMode::Off,
                Some(FlagOrAuto::Flag(true)) => PresetMode::On,
                Some(FlagOrAuto::Flag(false)) => PresetMode::Off,
                Some(FlagOrAuto::Text(text)) => PresetMode::parse(&text).with_context(|| {
                    format!("{key}: expected true, false or auto, got '{text}'")
                })?,
            },
        })
    }

    /// Bases de données : drapeau, période et chemins de sauvegarde par moteur.
    pub(super) fn merge_databases(
        file: DatabasesFile,
        env: &EnvSource,
        wanted: bool,
    ) -> Result<DatabasesConfig> {
        let enabled = match env.get("DUMBMONIT_AGENT_DATABASES") {
            Some(raw) => parse_bool(&raw).context("DUMBMONIT_AGENT_DATABASES")?,
            None => file.enabled.unwrap_or(wanted),
        };
        let interval = Duration::from_secs(read_interval(
            env,
            "DUMBMONIT_AGENT_DATABASES_INTERVAL_SECS",
            file.interval_secs,
            databases::DEFAULT_INTERVAL_SECS,
            databases::MIN_INTERVAL_SECS,
            "the database reading period",
        )?);
        let mut backups = Vec::new();
        for (key, entry) in file.backups {
            let Some(engine) = Engine::parse(&key) else {
                let known: Vec<&str> = Engine::ALL.iter().map(|e| e.as_str()).collect();
                bail!("databases.backups: unknown engine '{key}' (known: {})", known.join(", "));
            };
            let paths: Vec<String> = entry
                .paths
                .into_iter()
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty())
                .collect();
            if paths.is_empty() {
                bail!("databases.backups.{key}: at least one path is needed");
            }
            if paths.len() > databases::MAX_PATHS_PER_ENGINE {
                bail!("databases.backups.{key}: at most {} paths", databases::MAX_PATHS_PER_ENGINE);
            }
            let max_age_hours = entry.max_age_hours.unwrap_or(databases::DEFAULT_MAX_AGE_HOURS);
            if max_age_hours == 0 {
                bail!("databases.backups.{key}: max_age_hours must be at least 1");
            }
            backups.push(BackupRule { engine, paths, max_age_hours });
        }
        Ok(DatabasesConfig { enabled, interval, backups })
    }

    pub(super) fn merge_system_health(
        file: SystemHealthFile,
        env: &EnvSource,
        wanted: bool,
    ) -> Result<SystemHealthConfig> {
        let flag = |variable: &str, from_file: Option<bool>, default: bool| -> Result<bool> {
            match env.get(variable) {
                Some(raw) => parse_bool(&raw).with_context(|| format!("{variable}: {raw}")),
                None => Ok(from_file.unwrap_or(default)),
            }
        };
        let updates_interval_secs = match env.get("DUMBMONIT_AGENT_UPDATES_INTERVAL_SECS") {
            Some(raw) => raw.trim().parse::<u64>().with_context(|| {
                format!("DUMBMONIT_AGENT_UPDATES_INTERVAL_SECS: invalid value '{raw}'")
            })?,
            None => file.updates_interval_secs.unwrap_or(DEFAULT_UPDATES_INTERVAL_SECS),
        };
        if updates_interval_secs < MIN_UPDATES_INTERVAL_SECS {
            bail!(
                "the pending-updates check period must be at least {MIN_UPDATES_INTERVAL_SECS} seconds"
            );
        }
        Ok(SystemHealthConfig {
            enabled: flag("DUMBMONIT_AGENT_SYSTEM_HEALTH", file.enabled, wanted)?,
            check_updates: flag("DUMBMONIT_AGENT_CHECK_UPDATES", file.check_updates, true)?,
            updates_interval: Duration::from_secs(updates_interval_secs),
            check_reboot: flag("DUMBMONIT_AGENT_CHECK_REBOOT", file.check_reboot, true)?,
            check_failed_units: flag(
                "DUMBMONIT_AGENT_CHECK_FAILED_UNITS",
                file.check_failed_units,
                true,
            )?,
        })
    }
}
