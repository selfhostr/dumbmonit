-- Fin de la musique du mode mur (Spotify, lien partagé, enceinte) : les deux
-- tables créées par `0035_music.sql` n'ont plus de lecteur.
DROP TABLE IF EXISTS music_spotify;
DROP TABLE IF EXISTS music_wall_link;
DELETE FROM settings WHERE key LIKE 'music.%';
