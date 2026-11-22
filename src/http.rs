//! Client HTTP minimal, limité à 127.0.0.1, pour interroger les outils IA locaux.
//! Écrit à la main : il lit la réponse jusqu'à la fermeture et tolère un en-tête
//! « chunked » suivi d'un corps brut (défaut constaté sur le serveur MCP de WanGP).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct Reponse {
    pub statut: u16,
    pub entetes: String,
    pub corps: String,
}

impl Reponse {
    pub fn entete(&self, nom: &str) -> Option<String> {
        let nom = nom.to_ascii_lowercase();
        self.entetes.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().to_ascii_lowercase() == nom).then(|| v.trim().to_string())
        })
    }
}

pub fn requete(
    port: u16,
    methode: &str,
    chemin: &str,
    entetes: &[(&str, &str)],
    corps: Option<&str>,
    delai: Duration,
) -> Option<Reponse> {
    let adresse = SocketAddr::from(([127, 0, 0, 1], port));
    let mut flux = TcpStream::connect_timeout(&adresse, Duration::from_millis(300)).ok()?;
    flux.set_read_timeout(Some(delai)).ok()?;
    flux.set_write_timeout(Some(delai)).ok()?;
    let mut req = format!(
        "{methode} {chemin} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\nUser-Agent: moniteur-ia\r\n"
    );
    for (k, v) in entetes {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(c) = corps {
        req.push_str(&format!("Content-Length: {}\r\n", c.len()));
    }
    req.push_str("\r\n");
    if let Some(c) = corps {
        req.push_str(c);
    }
    flux.write_all(req.as_bytes()).ok()?;

    let mut brut = Vec::new();
    let mut tampon = [0u8; 16384];
    loop {
        match flux.read(&mut tampon) {
            Ok(0) => break,
            Ok(n) => {
                brut.extend_from_slice(&tampon[..n]);
                if brut.len() > 4 * 1024 * 1024 {
                    break;
                }
            }
            Err(_) => break, // délai dépassé : on garde ce qui est arrivé
        }
    }
    let texte = String::from_utf8_lossy(&brut).into_owned();
    let (tete, corps) = texte.split_once("\r\n\r\n")?;
    let statut = tete.split_whitespace().nth(1)?.parse().ok()?;
    let mut rep = Reponse { statut, entetes: tete.to_string(), corps: corps.to_string() };
    if rep
        .entete("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        if let Some(d) = dechunker(&rep.corps) {
            rep.corps = d;
        }
    }
    Some(rep)
}

/// Décode un corps « chunked » ; renvoie None si le corps n'en est pas un.
fn dechunker(s: &str) -> Option<String> {
    let mut reste = s;
    let mut sortie = String::new();
    loop {
        let (ligne, apres) = reste.split_once("\r\n")?;
        let taille = usize::from_str_radix(ligne.split(';').next()?.trim(), 16).ok()?;
        if taille == 0 {
            return Some(sortie);
        }
        if apres.len() < taille {
            sortie.push_str(apres);
            return Some(sortie);
        }
        sortie.push_str(apres.get(..taille)?);
        reste = apres.get(taille..)?.trim_start_matches("\r\n");
    }
}

/// Extrait le JSON d'une réponse, qu'elle soit en JSON simple ou en flux SSE (« data: … »).
pub fn json(corps: &str) -> Option<serde_json::Value> {
    let t = corps.trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Some(v);
    }
    t.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .filter_map(|d| serde_json::from_str(d.trim()).ok())
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunked_et_sse() {
        assert_eq!(dechunker("5\r\nhello\r\n0\r\n\r\n").unwrap(), "hello");
        assert!(dechunker("{\"a\":1}").is_none());
        assert_eq!(json("event: message\ndata: {\"x\":2}\n\n").unwrap()["x"], 2);
    }
}
