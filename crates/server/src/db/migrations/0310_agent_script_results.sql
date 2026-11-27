-- Contrôles personnalisés exécutés par un agent (`kind = 'agent_script'`) :
-- le dernier verdict de chacun.
--
-- Une ligne naît quand le contrôle est confié à son agent (accusé de réception
-- d'un lot), et se remplit quand l'agent rend un verdict. C'est elle que relit
-- le collecteur à chaque interrogation : fraîcheur, statut, message. Les
-- mesures, elles, partent dans VictoriaMetrics dès la réception.
--
-- Rien ici n'est un réglage : la ligne se reconstruit au lot suivant, elle
-- n'entre donc pas dans la sauvegarde.
CREATE TABLE agent_script_results (
    target_id      INTEGER PRIMARY KEY REFERENCES targets(id) ON DELETE CASCADE,
    -- Machine à qui le contrôle a été confié ; un changement d'agent efface le
    -- verdict de l'ancien.
    agent_id       INTEGER NOT NULL,
    -- Première remise du contrôle à cet agent (ms depuis l'époque).
    offered_at_ms  INTEGER NOT NULL,
    -- Réception du dernier verdict, à l'heure du serveur ; NULL tant qu'aucun.
    received_at_ms INTEGER,
    -- Début de l'exécution, à l'heure de l'agent.
    ran_at_ms      INTEGER,
    -- 0 OK, 1 WARNING, 2 CRITICAL, 3 UNKNOWN (codes Nagios).
    status         INTEGER CHECK (status BETWEEN 0 AND 3),
    message        TEXT    NOT NULL DEFAULT '',
    -- 'refused', 'timeout' ou 'failed' quand le contrôle n'a pas rendu de verdict.
    error          TEXT,
    duration_ms    INTEGER
) WITHOUT ROWID;

CREATE INDEX agent_script_results_agent ON agent_script_results (agent_id);
