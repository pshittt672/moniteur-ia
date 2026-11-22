# Moniteur IA — version 0.4.0

Petit moniteur temps réel (GPU, VRAM, CPU, RAM, WanGP, Forge Neo, LM Studio), en Rust natif (egui 0.34, rendu logiciel par le processeur : aucun pilote graphique chargé).

## Utiliser
- `moniteur-ia.exe` : fenêtre complète.
- `moniteur-ia.exe --compact` : barre flottante toujours au premier plan (double-clic pour agrandir).
  La barre se déplace en la faisant glisser ; sa position est mémorisée dans `position-compacte.txt`
  (à côté du .exe, contenu « x y » en pixels écran) et retrouvée au lancement suivant.
  Supprimer ce fichier rétablit la position par défaut.

## Boutons
- Mode vidéo : lance scripts\mode-video.ps1 (arrête Forge Neo, libère la RAM de Docker).
- Mode images : lance scripts\mode-images.ps1 (redémarre Forge Neo).
- Mode repos : arrête Forge Neo (via mode-video.ps1) et WanGP seulement si sa file est vide et le GPU sous 30 %.
Chaque bouton demande un second clic de confirmation.

## Construire sur le PC (Rust installé)
```powershell
cd C:\Dev\moniteur-ia
cargo build --release
```
Résultat : `target\release\moniteur-ia.exe`. (L'icône du .exe n'est ajoutée que si `windres` est disponible ; la fenêtre a toujours son icône.)

## Fichiers
- `src\main.rs` : interface ; `src\mesures.rs` : relevés ; `src\ia.rs` : WanGP / Forge / LM Studio ;
  `src\http.rs` : client HTTP local (127.0.0.1 seulement) ; `src\compteurs.rs` : compteurs Windows.
- Données fictives pour tester l'affichage : variable d'environnement `MONITEUR_DEMO=1`.
- `moniteur-ia.exe --diagnostic` : écrit un relevé texte dans `diagnostic.txt` à côté du .exe.

## Points à vérifier
- Température CPU : zone thermique ACPI, approximative.
