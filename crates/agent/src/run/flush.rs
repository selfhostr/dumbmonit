//! Envoi des lots, tampon d'attente, liaison et période d'échantillonnage.

use super::*;

impl Agent {
    /// Envoie un lot prélevé dans le tampon.
    pub(super) async fn flush(&mut self) -> FlushOutcome {
        self.ensure_tls().await;
        if self.tls_only() && !self.client.is_encrypted() {
            // Jamais en clair une fois le canal chiffré exigé localement : les
            // mesures attendent l'enrôlement.
            return FlushOutcome::Failed(PushError::Transport(
                "waiting for the encrypted agent channel (enrolment pending)".to_string(),
            ));
        }
        let samples = self.buffer.take(self.batch_limit);
        if samples.is_empty() {
            return FlushOutcome::Idle;
        }
        let count = samples.len();
        if self.gateway.is_some() {
            self.identity.relay_gateway_check =
                self.gateway_check.lock().ok().and_then(|held| held.clone());
        }
        // Le canal chiffré arrive après l'enrôlement : la passerelle le dit dès
        // le lot qui suit, sans attendre de changer d'adresse.
        if let Some(state) = self.identity.relay_gateway_state.as_mut() {
            state.encrypted = self.client.is_encrypted();
        }
        // Par la WebSocket, l'adresse de l'interface (`wss://…`), pas le pont.
        self.identity.server_link = self.failover.link(&self.client.endpoint());
        let mut batch =
            PushBatch::new(self.identity.clone(), samples, chrono::Utc::now().timestamp_millis());
        if self.wants_inventory() {
            batch.inventory = inventory::lock(&self.inventory).clone();
        }
        batch.event_log = self.eventlog.report();
        batch.declared_monitors = self.declared.clone();
        batch.script_results = self.scripts.pending();

        match self.client.send(&batch).await {
            Ok(ack) => {
                if let Some((since, _)) = self.outage.take() {
                    info!(
                        attempts = self.backoff.attempts(),
                        outage_s = since.elapsed().as_secs(),
                        "connection to the server restored"
                    );
                }
                self.backoff.reset();
                self.buffer.commit();
                self.buffer.resume();
                self.failover.succeeded();
                self.binding_refused = false;
                self.note_approved();
                debug!(target = ack.target_id, accepted = ack.accepted, "batch accepted");
                self.note_acceptance(count, ack.accepted);
                self.forget_inventory(batch.inventory.as_ref());
                self.eventlog.acknowledge(batch.event_log.as_ref());
                self.scripts.acknowledge(batch.script_results.as_deref());
                self.adopt_binding(ack.agent_secret.clone(), ack.bound);
                self.adopt_interval(ack.interval_secs);
                self.adopt_disabled_modules(ack.disabled_modules);
                self.adopt_relay(ack.relay);
                // `None` : serveur antérieur, ou machine qui n'est pas sous
                // Windows ; les règles connues restent.
                if let Some(rules) = ack.event_rules {
                    self.eventlog.set_rules(rules);
                }
                self.server_logs = ack.logs;
                // `None` : serveur antérieur, les contrôles connus restent.
                if let Some(checks) = ack.script_checks {
                    self.scripts.set_checks(checks);
                }
                // `None` : serveur antérieur. Un serveur qui répond sans réglage
                // ne retire rien à une sentinelle déjà en marche.
                if ack.sentinel.is_some() {
                    self.server_sentinel = ack.sentinel;
                }
                // Nouveau binaire à l'essai : ce lot accepté le confirme.
                if self.updater.confirm() {
                    self.identity.self_update = Some(self.updater.info());
                }
                self.update_offer = ack.agent_update;
                FlushOutcome::Sent { count }
            }
            Err(PushError::Forbidden(message)) if message == dumbmonit_proto::MTLS_REQUIRED => {
                // L'ancien canal est fermé : l'annonce qui accompagne ce refus
                // déclenche la migration au prochain tour.
                self.buffer.restore(batch.samples);
                self.tls_retry_at = None;
                info!("the server requires the encrypted agent channel, switching");
                FlushOutcome::Failed(PushError::Forbidden(message))
            }
            Err(PushError::Forbidden(message)) => {
                self.buffer.restore(batch.samples);
                self.on_binding_refused(&message);
                FlushOutcome::Failed(PushError::Forbidden(message))
            }
            Err(PushError::CertificateRefused) => {
                self.buffer.restore(batch.samples);
                self.on_certificate_refused();
                FlushOutcome::Failed(PushError::CertificateRefused)
            }
            Err(PushError::TooLarge(message)) => {
                if batch.inventory.is_some() {
                    // L'inventaire est le premier suspect : on le lâche pour ce
                    // jour (relevé suivant demain) plutôt que de tailler les mesures.
                    warn!(reason = message, "inventory too large for the server, dropped");
                    self.forget_inventory(batch.inventory.as_ref());
                    self.buffer.restore(batch.samples);
                    return FlushOutcome::Failed(PushError::TooLarge(message));
                }
                if batch.event_log.is_some() {
                    // Puis les événements du journal, que rien ne permet de couper.
                    warn!(reason = message, "event log report too large for the server, dropped");
                    self.eventlog.acknowledge(batch.event_log.as_ref());
                    self.buffer.restore(batch.samples);
                    return FlushOutcome::Failed(PushError::TooLarge(message));
                }
                if count <= 1 {
                    // Une mesure seule ne se coupe plus : la garder bloquerait
                    // tout le tampon derrière elle.
                    warn!(reason = message, "measurement too large for the server, dropped");
                    self.buffer.commit();
                    return FlushOutcome::Sent { count };
                }
                self.buffer.restore(batch.samples);
                self.batch_limit = (count / 2).max(1);
                warn!(
                    reason = message,
                    batch_limit = self.batch_limit,
                    "batch too large for the server or a proxy, sending smaller batches"
                );
                FlushOutcome::Failed(PushError::TooLarge(message))
            }
            Err(error) if error.is_retryable() => {
                // Journalisé par la boucle, qui connaît le délai avant la reprise.
                self.buffer.restore(batch.samples);
                if matches!(error, PushError::Transport(_)) {
                    self.note_unreachable(&error);
                }
                FlushOutcome::Failed(error)
            }
            Err(error) => {
                // Le serveur a compris et refusé : réémettre à l'identique ne ferait
                // que reproduire le refus, en gardant le tampon plein pour rien.
                warn!(%error, dropped = count, "batch rejected by the server, dropped");
                self.buffer.commit();
                self.forget_inventory(batch.inventory.as_ref());
                self.eventlog.acknowledge(batch.event_log.as_ref());
                FlushOutcome::Sent { count }
            }
        }
    }

    /// Fichier des mesures en attente gardées à l'arrêt, à côté du secret de
    /// liaison : le seul répertoire où l'agent sait pouvoir écrire.
    pub(super) fn pending_path(&self) -> std::path::PathBuf {
        self.config.secret_path.with_file_name("pending-samples.json")
    }

    /// Reprend les mesures qu'un arrêt précédent n'avait pas pu envoyer.
    pub(super) fn restore_pending(&mut self) {
        let path = self.pending_path();
        let samples = PendingBuffer::load(&path);
        if !samples.is_empty() {
            info!(count = samples.len(), "measurements kept at the last shutdown, sending them");
            self.buffer.push(samples);
        }
    }

    /// Signale un lot non vide dont le serveur n'a gardé aucune mesure.
    ///
    /// Le serveur écarte sans erreur les mesures hors de `[maintenant - 30 j,
    /// maintenant + 1 h]` : une horloge déréglée sur la machine les fait toutes
    /// disparaître, et rien ne le disait. Renvoie vrai si l'avertissement part.
    pub(super) fn note_acceptance(&mut self, sent: usize, accepted: usize) -> bool {
        if sent == 0 || accepted > 0 {
            self.all_refused = false;
            return false;
        }
        if self.all_refused {
            return false;
        }
        self.all_refused = true;
        warn!(
            sent,
            "the server kept none of the measurements of this batch: check this machine's \
             clock (timestamps more than 1 hour ahead or 30 days behind the server are discarded)"
        );
        true
    }

    /// Dernière tentative d'envoi avant de rendre la main au système.
    pub(super) async fn final_flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        // Sur le disque d'abord : un envoi coupé par le délai n'emporte alors
        // rien avec lui.
        self.buffer.spill();
        if let Ok(FlushOutcome::Sent { count }) =
            tokio::time::timeout(SHUTDOWN_FLUSH_TIMEOUT, self.flush()).await
        {
            info!(count, "pending measurements sent before shutdown");
        }
        if self.buffer.is_empty() {
            return;
        }
        let pending = self.buffer.len();
        if self.buffer.has_spool() {
            info!(
                pending,
                bytes = self.buffer.disk_bytes(),
                "the server did not answer before shutdown: measurements kept in the disk buffer"
            );
            return;
        }
        // Sans tampon disque : ce qui reste attend le prochain démarrage dans
        // un fichier plutôt que d'être perdu.
        let path = self.pending_path();
        match self.buffer.save(&path, MAX_PERSISTED_SAMPLES) {
            Ok(kept) => info!(
                kept,
                lost = pending.saturating_sub(kept),
                path = %path.display(),
                "the server did not answer before shutdown: measurements kept on disk"
            ),
            Err(error) => warn!(
                pending,
                %error,
                "measurements lost: the server did not answer before shutdown"
            ),
        }
    }

    /// Range le secret de liaison que le serveur vient d'attribuer.
    ///
    /// Il n'arrive qu'une fois, dans l'accusé de réception du lot qui a lié la
    /// machine. Tout ce qui suit le présentera à chaque requête.
    pub(super) fn adopt_binding(&mut self, issued: Option<String>, bound: bool) {
        let Some(secret) = issued else {
            if !bound {
                // Le serveur nous connaît sans nous avoir liés : c'est le cas
                // d'un enregistrement d'avant la liaison, pas une anomalie.
                debug!("this machine is not bound to this agent installation yet");
            }
            return;
        };
        self.binding.save(&secret);
        self.client.set_secret(Some(secret));
        info!("this machine is now bound to this agent installation");
    }

    /// Le serveur ne reconnaît pas notre liaison — ou quelqu'un a répondu 403.
    ///
    /// Le secret n'est oublié que sur le refus de liaison du serveur lui-même :
    /// le garder n'ouvre rien, et repartir sans lui est ce qu'il faut faire dès
    /// que quelqu'un aura cliqué « Allow re-enrolment » dans l'interface. Tout
    /// autre 403 — mandataire inverse, pare-feu applicatif — ne dit rien de la
    /// liaison : effacer le secret sur sa foi obligerait à réenrôler chaque
    /// machine à la main. Les mesures restent en tampon dans les deux cas,
    /// l'agent réessaie avec sa temporisation habituelle.
    pub(super) fn on_binding_refused(&mut self, message: &str) {
        if message == dumbmonit_proto::ENROLMENT_PENDING {
            self.note_awaiting_approval();
            return;
        }
        if message == dumbmonit_proto::BINDING_MISMATCH {
            self.binding.forget();
            self.client.set_secret(None);
        }
        if self.binding_refused {
            debug!(reason = message, "still refused by the server");
            return;
        }
        self.binding_refused = true;
        warn!(reason = message, "this agent is not recognised for this machine");
    }

    /// Le serveur fait attendre l'enrôlement : un administrateur doit accepter
    /// la machine. Dit une fois, puis en `debug` à chaque nouvelle tentative.
    pub(super) fn note_awaiting_approval(&mut self) {
        if self.awaiting_approval {
            debug!("still waiting for approval");
            return;
        }
        self.awaiting_approval = true;
        info!(
            "waiting for approval: an administrator must accept this machine in DumbMonit \
             (Settings > Agents > Waiting for approval); retrying on its own"
        );
    }

    /// Fin de l'attente de validation, si l'agent attendait.
    pub(super) fn note_approved(&mut self) {
        if std::mem::take(&mut self.awaiting_approval) {
            info!("enrolment approved by the server");
        }
    }

    /// Adopte la période demandée par le serveur.
    ///
    /// C'est ce qui permet de régler la finesse d'échantillonnage d'une machine
    /// depuis l'interface, sans se connecter dessus ni redémarrer l'agent.
    pub(super) fn adopt_interval(&mut self, wanted_secs: u64) {
        let wanted = Duration::from_secs(wanted_secs.max(MIN_INTERVAL_SECS));
        if wanted != self.interval {
            info!(
                old_s = self.interval.as_secs(),
                new_s = wanted.as_secs(),
                "sampling period adjusted by the server"
            );
            self.interval = wanted;
        }
    }
}
