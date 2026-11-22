# Moniteur IA

**Moniteur matériel léger pour Windows, pensé pour suivre son PC pendant l'utilisation d'outils d'IA locaux** (génération d'images, de vidéos, modèles de langage).

Il affiche en temps réel l'occupation du GPU, de la VRAM, du processeur et de la mémoire, ainsi que l'état de trois outils : **WanGP** (vidéo), **Forge Neo** (images) et **LM Studio** (modèles de langage). Une barre compacte, toujours au premier plan, permet de garder un œil sur la machine pendant un rendu.

> **Configuration requise** : Windows 10 ou 11, 64 bits. Une carte graphique **NVIDIA** est nécessaire pour les mesures GPU et VRAM.

---

## Points forts

- **Léger** : environ 30 Mo de mémoire en barre compacte, 46 Mo en fenêtre complète, et 1 à 3 % d'un cœur de processeur. Mesures faites sur un PC portable avec une RTX 4060.
- **Discret pour le GPU** : l'affichage est calculé par le processeur. Le moniteur ne charge **aucun pilote graphique** et ne prend donc ni VRAM ni temps de calcul aux outils d'IA.
- **Un seul fichier** : un exe de quelques Mo, sans installation, sans service et sans écriture dans le registre.
- **Local uniquement** : il interroge seulement `127.0.0.1` et n'envoie rien sur Internet.

## Ce qu'il affiche

| Zone | Contenu | Source |
|---|---|---|
| **GPU** | Utilisation, température, consommation et limite, fréquence | Pilote NVIDIA (NVML) |
| **VRAM** | Mémoire vidéo utilisée et totale | Pilote NVIDIA (NVML) |
| **CPU** | Utilisation, nombre de cœurs, température approximative | Windows (zone thermique ACPI) |
| **RAM** | Mémoire utilisée, fichier d'échange, mémoire de Docker / WSL | Windows (compteurs de performance) |
| **Activité IA** | État de WanGP, Forge Neo et LM Studio (voir ci-dessous) | Interfaces locales des outils |
| **Processus** | Programmes les plus gourmands (CPU, VRAM, RAM) | Windows, pilote NVIDIA |

### Détection des outils d'IA

| Outil | Comment il est lu | Ce qui s'affiche |
|---|---|---|
| **WanGP** | Processus `python` lancé avec `wgp.py`, et serveur MCP sur `http://127.0.0.1:7866/mcp` | Arrêté, en attente ou en cours ; nombre de tâches en cours et en attente |
| **Forge Neo** | API sur `http://127.0.0.1:7861` | Arrêté, prêt, ou génération en cours avec l'étape et le temps restant |
| **LM Studio** | Serveur local sur `http://127.0.0.1:1234` | Fermé, ouvert, ou modèle chargé en mémoire |

Un outil absent apparaît simplement comme « Arrêté » ou « Fermé », sans erreur.

## Utilisation

1. Téléchargez `moniteur-ia.exe` depuis la page **Releases** du dépôt.
2. Placez-le dans un dossier de votre choix, par exemple `C:\Outils\moniteur-ia\`.
3. Lancez-le. L'exe des releases est signé numériquement (certificat de signature de code HARICA, avec horodatage) : l'éditeur affiché par Windows est « Yann TANGUY ». Tant que le fichier est récent, Windows SmartScreen peut encore afficher un avertissement, le temps que sa réputation s'établisse. Cliquez alors sur « Informations complémentaires », puis sur « Exécuter quand même ».

| Commande | Effet |
|---|---|
| `moniteur-ia.exe` | Fenêtre complète |
| `moniteur-ia.exe --compact` | Barre compacte flottante, toujours au premier plan |
| `moniteur-ia.exe --diagnostic` | Écrit un relevé texte dans `diagnostic.txt`, à côté de l'exe, puis se ferme |

Pour un accès rapide, créez un raccourci vers l'exe, et un second raccourci avec `--compact` à la fin de la cible.

### Barre compacte

- **Déplacer** : cliquez sur la barre en dehors des boutons et faites-la glisser.
- **Position mémorisée** : la position est enregistrée dans `position-compacte.txt`, à côté de l'exe, puis reprise au lancement suivant. Si cette position n'est plus sur un écran, la barre revient à sa place par défaut. Supprimez le fichier pour réinitialiser la position.
- **Largeur** : la barre prend la largeur strictement nécessaire à son contenu, et reste stable quand les valeurs changent.
- **Revenir à la fenêtre complète** : bouton d'agrandissement, ou double-clic sur la barre.

### Boutons de la fenêtre complète

- **Toujours au premier plan** : garde la fenêtre au-dessus des autres.
- **Mode compact** : passe à la barre flottante.
- **Mode repos, Mode images, Mode vidéo** : ces boutons lancent des scripts PowerShell propres à la configuration de l'auteur, rangés dans `C:\docker\ai-stack\scripts\`. **Ils sont masqués automatiquement si ces scripts n'existent pas**, ce qui est le cas sur tout autre PC.

## Fonctionnement

- **Deux fils de mesure** tournent en arrière-plan.
  - Le **fil matériel** lit le GPU, le processeur, la mémoire et les compteurs Windows **chaque seconde**. Les mesures plus coûteuses (liste des processus, mémoire de Docker / WSL, VRAM par processus) ne sont relevées que **toutes les 5 secondes**.
  - Le **fil IA** interroge les trois outils **toutes les 2 secondes**, avec un délai d'attente court, pour ne jamais bloquer l'affichage.
- **L'affichage** n'est redessiné qu'une fois par seconde, avec [egui](https://github.com/emilk/egui) et un rendu logiciel ([egui_software_backend](https://crates.io/crates/egui_software_backend)).
- **Le GPU** est lu par NVML, la bibliothèque fournie avec le pilote NVIDIA.
- **La température du processeur, le fichier d'échange et la mémoire de Docker / WSL** viennent des compteurs de performance Windows (PDH), lisibles sans droits administrateur. La mémoire de Docker / WSL correspond à celle des processus `vmmem`.

## Limites connues

- **Windows uniquement.**
- **Sans carte NVIDIA**, les zones GPU et VRAM indiquent « Pilote NVIDIA non lisible ». Le reste fonctionne.
- **Température du processeur** : elle vient d'une zone thermique ACPI. Selon les fabricants, elle est approximative, ou absente.
- **VRAM par processus** : Windows ne la fournit souvent pas, et la colonne affiche alors « — ».
- **Ports fixes** : 7866 pour WanGP, 7861 pour Forge Neo et 1234 pour LM Studio. Avec d'autres ports, l'outil apparaît comme arrêté. Pour les changer, modifiez les constantes au début de `src/ia.rs`, puis recompilez.

## Compiler depuis les sources

Il faut [Rust](https://rustup.rs/), avec la chaîne MSVC sous Windows.

```powershell
git clone https://github.com/pshittt672/moniteur-ia.git
cd moniteur-ia
cargo build --release
```

Le programme est produit dans `target\release\moniteur-ia.exe`. L'icône n'est intégrée à l'exe que si `windres` est disponible ; la fenêtre a toujours son icône.

Pour tester l'affichage avec des données fictives, définissez la variable d'environnement `MONITEUR_DEMO=1` avant de lancer le programme.

### Organisation du code

| Fichier | Rôle |
|---|---|
| `src/main.rs` | Interface : fenêtre complète, barre compacte, boutons |
| `src/mesures.rs` | Relevés matériels et fils de mesure |
| `src/ia.rs` | Lecture de WanGP, Forge Neo et LM Studio |
| `src/http.rs` | Petit client HTTP limité à `127.0.0.1` |
| `src/compteurs.rs` | Compteurs de performance Windows (PDH) |

Les versions publiées sont compilées par GitHub Actions (`.github/workflows/release.yml`), à partir de ces sources, à chaque étiquette `v*`.
