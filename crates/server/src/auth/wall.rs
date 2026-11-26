//! Le rôle `wall` : un compte d'écran mural, qui ne voit que le mode mur.
//!
//! Un écran fixé au mur (une télévision de salle d'exploitation, un écran
//! d'accueil) se connecte avec ce compte et n'affiche que `/wall`. Le garde de
//! session ([`super::middleware::require_session`]) ne lui ouvre que la liste
//! blanche [`WALL_ROUTES`] ; tout le reste lui répond 403, lectures comprises.
//!
//! Le reste du régime d'un écran mural, rangé là où il s'applique :
//! - sa session se renouvelle tant qu'il affiche le mur (`session::touch`) ;
//! - la politique « second facteur obligatoire » ne le vise pas (`policy`) ;
//! - ses jetons d'API n'ouvrent rien (`token::authenticate`) ;
//! - la liste des équipements lui arrive sans adresse ni options
//!   (`api::targets`), et ses équipes limitent ce qu'il montre.

use axum::http::Method;

use crate::auth::teams;

/// Les seules routes qu'un compte d'écran mural (rôle `wall`) peut appeler :
/// ce que lit la page `/wall`, plus la déconnexion. Méthode et gabarit de
/// chemin sans `/api`, `*` = un segment non vide, comme
/// [`super::middleware::OPERATOR_ROUTES`].
///
/// Liste blanche stricte, refus par défaut : une route ajoutée demain reste
/// fermée au mur tant qu'on ne l'a pas inscrite ici. Toutes sont des lectures,
/// sauf la déconnexion (écriture « pour soi » du garde).
/// Les listes (équipements, alertes, règles, métriques) sont filtrées par le
/// gestionnaire selon les équipes du compte, et le garde des équipes
/// s'applique ensuite comme pour tout compte restreint
/// ([`teams::SCOPED_ROUTES`]).
///
/// Ce n'est pas la page de statut publique (`/s/{slug}`, `/api/public/…`) :
/// celle-ci est anonyme et ne montre que ce qu'un administrateur y a mis ; le
/// compte mural, lui, voit le mur de la console, avec le parc entier ou celui
/// de ses équipes.
pub const WALL_ROUTES: &[(&str, &str)] = &[
    // Le compte lui-même : qui suis-je, et se déconnecter.
    ("GET", "/auth/me"),
    ("POST", "/auth/logout"),
    // Le nom de l'instance, affiché en tête du mur.
    ("GET", "/instance/settings"),
    // L'état du parc : équipements, alertes, règles (pour le verdict), sondes
    // et chiffres du bandeau (métriques).
    ("GET", "/targets"),
    ("GET", "/alerts"),
    ("GET", "/alerts/rules"),
    ("GET", "/metrics/query"),
    ("GET", "/metrics/query_range"),
    // La ligne sous le verdict : incident publié en cours, ou fenêtre de
    // maintenance qui couvre l'instant.
    ("GET", "/incidents"),
    ("GET", "/alerts/silences"),
    // La météo du dehors, à côté de la date.
    ("GET", "/wall/weather"),
];

/// Message du refus : l'interface d'un écran mural ne l'affiche pas, mais un
/// script qui se tromperait de compte comprend pourquoi.
pub const WALL_ONLY_MESSAGE: &str = "This account is a wall display: it can only show the wall.";

/// Vrai si `method path` fait partie de ce qu'un écran mural peut appeler.
pub fn is_wall_route(method: &Method, path: &str) -> bool {
    teams::route_listed(WALL_ROUTES, method, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_display_reaches_the_wall_and_nothing_else() {
        let yes = |m: Method, p: &str| assert!(is_wall_route(&m, p), "{m} {p}");
        let no = |m: Method, p: &str| assert!(!is_wall_route(&m, p), "{m} {p}");
        yes(Method::GET, "/api/targets");
        yes(Method::GET, "/targets/");
        yes(Method::HEAD, "/alerts");
        yes(Method::GET, "/alerts/rules");
        yes(Method::GET, "/alerts/silences");
        yes(Method::GET, "/incidents");
        yes(Method::GET, "/metrics/query_range");
        yes(Method::GET, "/wall/weather");
        yes(Method::POST, "/api/auth/logout");
        yes(Method::GET, "/auth/me");

        no(Method::POST, "/targets");
        no(Method::GET, "/targets/1");
        no(Method::GET, "/targets/1/rules");
        no(Method::GET, "/alerts/history");
        no(Method::POST, "/alerts/abc/ack");
        no(Method::POST, "/alerts/silences");
        no(Method::DELETE, "/alerts/silences/3");
        no(Method::POST, "/incidents");
        no(Method::GET, "/incidents/1");
        no(Method::GET, "/metrics/catalog");
        no(Method::PUT, "/wall/weather");
        no(Method::GET, "/wall/weather/places");
        no(Method::POST, "/auth/password");
        no(Method::GET, "/auth/sessions");
        no(Method::POST, "/auth/totp/enroll");
        no(Method::GET, "/users");
        no(Method::GET, "/tokens");
        no(Method::GET, "/backup");
        no(Method::GET, "/notify/channels");
        no(Method::GET, "/summary");
        no(Method::PUT, "/instance/settings");
        no(Method::GET, "/auth/me/extra");
        no(Method::GET, "/authors");
    }
}
