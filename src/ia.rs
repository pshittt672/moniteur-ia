//! État des outils IA locaux : WanGP (serveur MCP, port 7866),
//! Forge Neo (API, port 7861) et LM Studio (serveur local, port 1234).

use crate::http::{json, requete};
use crate::mesures::{Niveau, Outil};
use std::time::Duration;

const PORT_WANGP: u16 = 7866;
const CHEMIN_MCP: &str = "/mcp";
const PORT_FORGE: u16 = 7861;
const PORT_LMSTUDIO: u16 = 1234;
const DELAI: Duration = Duration::from_millis(1500);

fn duree(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    if s >= 60 { format!("{} min {:02}", s / 60, s % 60) } else { format!("{s} s") }
}

// ---------------------------------------------------------------- Forge Neo
pub fn forge() -> Outil {
    let Some(r) = requete(PORT_FORGE, "GET", "/sdapi/v1/progress?skip_current_image=true", &[], None, DELAI)
    else {
        return Outil {
            niveau: Niveau::Arrete,
            detail: "API non joignable (conteneur arrêté ?)".into(),
            etat: "Arrêté".into(),
            ..Default::default()
        };
    };
    let Some(v) = json(&r.corps).filter(|_| r.statut == 200) else {
        return Outil {
            niveau: Niveau::Inactif,
            detail: format!("Démarrage en cours (réponse {})", r.statut),
            etat: "Démarrage".into(),
            ..Default::default()
        };
    };
    let p = v["progress"].as_f64().unwrap_or(0.0);
    let eta = v["eta_relative"].as_f64().unwrap_or(0.0);
    let st = &v["state"];
    let (pas, total) = (st["sampling_step"].as_u64().unwrap_or(0), st["sampling_steps"].as_u64().unwrap_or(0));
    let travaux = st["job_count"].as_i64().unwrap_or(0);
    if p > 0.0 || travaux > 0 {
        let mut detail = if total > 0 { format!("Étape {pas}/{total}") } else { "Génération".into() };
        if travaux > 1 {
            detail.push_str(&format!(" · {travaux} images dans le lot"));
        }
        Outil {
            niveau: Niveau::Actif,
            progression: Some(p as f32),
            detail,
            etat: if eta > 0.0 { format!("En cours · reste ~{}", duree(eta)) } else { "En cours".into() },
            ..Default::default()
        }
    } else {
        Outil {
            niveau: Niveau::Inactif,
            detail: "Prêt · aucune génération".into(),
            etat: "En attente".into(),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------- LM Studio
pub fn lmstudio(processus_lance: bool) -> Outil {
    let rep = requete(PORT_LMSTUDIO, "GET", "/api/v0/models", &[], None, DELAI);
    let modeles: Option<Vec<String>> = rep.and_then(|r| json(&r.corps)).map(|v| {
        v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|m| m["state"] == "loaded")
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    });
    match (processus_lance, modeles) {
        (_, Some(m)) if !m.is_empty() => Outil {
            niveau: Niveau::Alerte,
            detail: format!("Modèle chargé : {}", m.join(", ")),
            etat: "Modèle en mémoire".into(),
            ..Default::default()
        },
        (_, Some(_)) => Outil {
            niveau: Niveau::Inactif,
            detail: "Ouvert · aucun modèle chargé · serveur :1234".into(),
            etat: "Ouvert (inactif)".into(),
            ..Default::default()
        },
        (true, None) => Outil {
            niveau: Niveau::Inactif,
            detail: "Ouvert · serveur local arrêté (modèle non visible)".into(),
            etat: "Ouvert".into(),
            ..Default::default()
        },
        (false, None) => Outil {
            niveau: Niveau::Arrete,
            detail: "Fermé".into(),
            etat: "Fermé".into(),
            ..Default::default()
        },
    }
}

// ---------------------------------------------------------------- WanGP
// Même appel que C:\docker\ai-stack\scripts\lancer-forge-neo.ps1 : POST JSON-RPC
// direct (sans session), outil wangp_session, action list_queue en résumé.
const APPEL_FILE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wangp_session","arguments":{"action":"list_queue","arguments":{"summary_only":true}}}}"#;

fn appel_file() -> Option<crate::http::Reponse> {
    requete(
        PORT_WANGP,
        "POST",
        CHEMIN_MCP,
        &[("Content-Type", "application/json"), ("Accept", "application/json, text/event-stream")],
        Some(APPEL_FILE),
        DELAI,
    )
}

/// Cherche « "cle": nombre » dans le texte brut (y compris dans du JSON échappé).
fn nombre_apres(texte: &str, cle: &str) -> Option<usize> {
    let debut = texte.find(cle)? + cle.len();
    let reste = &texte[debut..];
    let reste = reste.trim_start_matches(|c: char| c == '\\' || c == '"' || c == ':' || c.is_whitespace());
    let chiffres: String = reste.chars().take_while(|c| c.is_ascii_digit()).collect();
    chiffres.parse().ok()
}

#[derive(Default)]
pub struct WanGP;

impl WanGP {
    /// (tâches en cours, tâches en attente), ou None si la file n'est pas lisible.
    fn file_attente(&mut self) -> Option<(usize, usize)> {
        let r = appel_file()?;
        Some((nombre_apres(&r.corps, "running_count")?, nombre_apres(&r.corps, "queued_count")?))
    }

    pub fn lire(&mut self, processus_lance: bool, gpu: Option<f32>) -> Outil {
        let mut o = Outil::default();
        if !processus_lance {
            o.niveau = Niveau::Arrete;
            o.detail = "Non lancé".into();
            o.etat = "Arrêté".into();
            return o;
        }
        let file = self.file_attente();
        let calcul = gpu.unwrap_or(0.0) >= 30.0;
        let mut d = Vec::new();
        match file {
            Some((0, 0)) => d.push("file vide".to_string()),
            Some((c, a)) => d.push(format!("en cours : {c} · en attente : {a}")),
            None => d.push("file non lisible (serveur MCP :7866)".into()),
        }
        if let Some(g) = gpu {
            d.push(format!("GPU {g:.0} %"));
        }
        o.file = file.map(|(c, a)| c + a);
        o.niveau = if calcul || file.is_some_and(|(c, _)| c > 0) { Niveau::Actif } else { Niveau::Inactif };
        o.detail = d.join(" · ");
        o.etat = if o.niveau == Niveau::Actif { "En cours".into() } else { "En attente".into() };
        o
    }
}

/// Réponse brute du serveur MCP de WanGP (mode --diagnostic).
pub fn diagnostic_wangp() -> String {
    match appel_file() {
        None => "WanGP MCP :7866 : pas de réponse (WanGP arrêté ?)\n".into(),
        Some(r) => format!(
            "--- WanGP list_queue : statut {}\n{}\n{}\n",
            r.statut,
            r.entetes,
            r.corps.chars().take(4000).collect::<String>()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compteurs_file() {
        let brut = r#"{"result":{"content":[{"type":"text","text":"{\"running_count\": 1, \"queued_count\":2}"}]}}"#;
        assert_eq!(nombre_apres(brut, "running_count"), Some(1));
        assert_eq!(nombre_apres(brut, "queued_count"), Some(2));
        assert_eq!(nombre_apres(r#"{"running_count":0,"queued_count":0}"#, "queued_count"), Some(0));
        assert_eq!(nombre_apres("rien", "queued_count"), None);
    }
}
