-- Modules facultatifs (`crate::features`, clé `features` de `settings`) : mode
-- mural, rapports SLA, pages de statut, SLO, carte des menaces. Coupés sur une
-- installation neuve ; une instance existante garde actif ce qu'elle utilise
-- déjà. La carte des menaces reste coupée (elle ne dépend d'aucune donnée à soi).
-- Les calendriers de rapports (digest compris) passent par les mêmes routes que
-- les rapports SLA : le moindre calendrier active le module.
INSERT INTO settings (key, value)
SELECT 'features', CAST(json_object(
    'wall', json(CASE WHEN EXISTS (SELECT 1 FROM users WHERE role = 'wall')
                        OR EXISTS (SELECT 1 FROM music_wall_link)
                      THEN 'true' ELSE 'false' END),
    'sla_reports', json(CASE WHEN EXISTS (SELECT 1 FROM report_schedules)
                             THEN 'true' ELSE 'false' END),
    'status_pages', json(CASE WHEN EXISTS (SELECT 1 FROM status_pages)
                              THEN 'true' ELSE 'false' END),
    'slo', json(CASE WHEN EXISTS (SELECT 1 FROM slos) THEN 'true' ELSE 'false' END),
    'threat_map', json('false')) AS BLOB)
WHERE NOT EXISTS (SELECT 1 FROM settings WHERE key = 'features');
