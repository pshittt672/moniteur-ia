// Moniteur IA : suivi en temps réel du GPU, de la VRAM, du CPU, de la RAM
// et de l'activité de WanGP, Forge Neo et LM Studio.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod compteurs;
mod http;
mod ia;
mod mesures;

use egui::{
    Align2, Color32, CornerRadius, FontData, FontFamily, FontId, Id, Pos2, Rect, Sense,
    Stroke, StrokeKind, Vec2, ViewportCommand, WindowLevel, epaint::PathStroke, pos2, vec2,
};
use mesures::{Etat, Niveau, Outil, Partage};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const FOND: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
const CARTE: Color32 = Color32::from_rgb(0x23, 0x23, 0x23);
const TRAIT: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
const TEXTE: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const DOUX: Color32 = Color32::from_rgb(0xa9, 0xa9, 0xa9);
const VERT: Color32 = Color32::from_rgb(0x2e, 0xe8, 0x8a);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0xb0, 0x20);
const ROUGE: Color32 = Color32::from_rgb(0xff, 0x4d, 0x6d);
const VIOLET: Color32 = Color32::from_rgb(0x8a, 0x2b, 0xe2);
const ROSE: Color32 = Color32::from_rgb(0xff, 0x1f, 0xbf);
const TAILLE_NORMALE: Vec2 = vec2(1120.0, 660.0);
const TAILLE_MIN: Vec2 = vec2(820.0, 540.0);
const TAILLE_COMPACTE: Vec2 = vec2(760.0, 40.0);
#[cfg(windows)]
const SCRIPTS: &str = r"C:\docker\ai-stack\scripts";

fn degrade(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    Color32::from_rgb(m(VIOLET.r(), ROSE.r()), m(VIOLET.g(), ROSE.g()), m(VIOLET.b(), ROSE.b()))
}

fn go(octets: u64) -> String {
    format!("{:.1}", octets as f64 / 1_073_741_824.0).replace('.', ",")
}

fn couleur_temp(t: f32, tiede: f32, chaud: f32) -> Color32 {
    if t >= chaud { ROUGE } else if t >= tiede { ORANGE } else { VERT }
}

// ------------------------------------------------------------------ polices
fn charger_polices(ctx: &egui::Context) {
    // Polices du système seulement (moins de mémoire) ; polices intégrées d'egui en secours.
    let mut defs = egui::FontDefinitions::empty();
    #[cfg(windows)]
    let (normal, gras) = (r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\segoeuib.ttf");
    #[cfg(not(windows))]
    let (normal, gras) = (
        "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
        "/usr/share/fonts/opentype/inter/InterDisplay-Bold.otf",
    );
    let mut ajouter = |nom: &str, chemin: &str| -> bool {
        match std::fs::read(chemin) {
            Ok(o) => {
                defs.font_data.insert(nom.into(), Arc::new(FontData::from_owned(o)));
                true
            }
            Err(_) => false,
        }
    };
    let a_normal = ajouter("normal", normal);
    let a_gras = ajouter("gras", gras);
    if !a_normal {
        defs = egui::FontDefinitions::default();
    }
    let prop = defs.families.entry(FontFamily::Proportional).or_default();
    if a_normal {
        prop.insert(0, "normal".into());
    }
    let mut famille_gras = prop.clone();
    if a_gras {
        famille_gras.insert(0, "gras".into());
    }
    defs.families.insert(FontFamily::Name("gras".into()), famille_gras);
    ctx.set_fonts(defs);
}

fn f(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Proportional)
}
fn g(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name("gras".into()))
}

// ------------------------------------------------------------------ application
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Video,
    Images,
    Repos,
}

struct App {
    etat: Partage,
    premier_plan: bool,
    compact: bool,
    confirmation: Option<(Mode, Instant)>,
    /// Dernier mode lancé depuis le moniteur, et quand.
    dernier_choix: Option<(Mode, Instant)>,
    message: Option<(String, Instant)>,
    compact_au_demarrage: bool,
    /// Passage en compact : moment où replacer la barre à sa dernière position.
    position_a_restaurer: Option<Instant>,
    /// Dernière largeur demandée pour la barre compacte (ajustée au contenu).
    largeur_compacte: f32,
}

impl App {
    fn lancer_mode(&mut self, mode: Mode, e: &Etat) {
        // Mode repos : arrêt de Forge (script mode vidéo) et de WanGP seulement s'il est inactif.
        let wangp_inactif = !e.wangp_pids.is_empty()
            && e.wangp.niveau != Niveau::Actif
            && e.wangp_gpu.unwrap_or(0.0) < 30.0
            && e.wangp.file == Some(0);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let fichier = match mode {
                Mode::Video | Mode::Repos => "mode-video.ps1",
                Mode::Images => "mode-images.ps1",
            };
            if mode == Mode::Repos && wangp_inactif {
                for pid in &e.wangp_pids {
                    let _ = std::process::Command::new("taskkill.exe")
                        .args(["/T", "/F", "/PID", &pid.to_string()])
                        .creation_flags(0x0800_0000)
                        .spawn();
                }
            }
            let chemin = std::path::Path::new(SCRIPTS).join(fichier);
            let r = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
                .arg(&chemin)
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .spawn();
            let texte = match (r, mode) {
                (Ok(_), Mode::Video) => "Mode vidéo lancé : arrêt de Forge et libération de la RAM…".to_string(),
                (Ok(_), Mode::Images) => "Mode images lancé : démarrage de Forge Neo…".to_string(),
                (Ok(_), Mode::Repos) if wangp_inactif => "Mode repos : arrêt de Forge Neo et de WanGP (inactif)…".to_string(),
                (Ok(_), Mode::Repos) if e.wangp_pids.is_empty() => "Mode repos : arrêt de Forge Neo…".to_string(),
                (Ok(_), Mode::Repos) => "Mode repos : Forge arrêté ; WanGP laissé ouvert (travail en cours ou file non lisible)".to_string(),
                (Err(e), _) => format!("Impossible de lancer {fichier} : {e}"),
            };
            self.message = Some((texte, Instant::now()));
        }
        #[cfg(not(windows))]
        {
            let _ = (mode, wangp_inactif);
            self.message = Some(("Disponible sous Windows uniquement".into(), Instant::now()));
        }
    }

    fn scripts_presents() -> bool {
        #[cfg(windows)]
        return std::path::Path::new(SCRIPTS).join("mode-video.ps1").exists();
        #[cfg(not(windows))]
        return true; // pour l'aperçu
    }

    fn basculer_compact(&mut self, ctx: &egui::Context) {
        self.compact = !self.compact;
        if self.compact {
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(vec2(150.0, 30.0)));
            ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(TAILLE_COMPACTE));
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
            // Non redimensionnable : empêche Windows d'agrandir la barre en plein écran
            // quand on la glisse contre le haut de l'écran (ancrage « Snap »).
            ctx.send_viewport_cmd(ViewportCommand::Resizable(false));
            self.position_a_restaurer = Some(Instant::now());
            self.largeur_compacte = 0.0; // recalculée au premier rendu compact
        } else {
            self.position_a_restaurer = None;
            ctx.send_viewport_cmd(ViewportCommand::Resizable(true));
            ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(TAILLE_MIN));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(TAILLE_NORMALE));
            self.appliquer_niveau(ctx);
        }
    }

    fn appliquer_niveau(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.premier_plan {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        }));
    }
}

/// Déplace la fenêtre à la souris (barre compacte sans barre de titre).
/// Sous Windows : on simule un clic sur la barre de titre, et Windows gère le déplacement
/// jusqu'au relâchement du bouton.
#[cfg(windows)]
fn deplacer_fenetre(_ctx: &egui::Context) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, ReleaseCapture};
    use windows_sys::Win32::UI::WindowsAndMessaging::{HTCAPTION, SendMessageW, WM_NCLBUTTONDOWN};
    unsafe {
        let fenetre = GetActiveWindow();
        if !fenetre.is_null() {
            ReleaseCapture();
            // Retourne quand le bouton est relâché : on mémorise alors la nouvelle position.
            SendMessageW(fenetre, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
            memoriser_position(fenetre);
        }
    }
}

#[cfg(not(windows))]
fn deplacer_fenetre(ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
}

/// Fichier « position-compacte.txt » à côté du .exe : « x y » (coin haut gauche, en pixels écran).
fn fichier_position() -> Option<std::path::PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("position-compacte.txt"))
}

#[cfg(windows)]
fn memoriser_position(fenetre: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsZoomed};
    // Fenêtre agrandie (plein écran) : ce n'est pas une position de barre, on ne l'enregistre pas.
    if unsafe { IsZoomed(fenetre) } != 0 {
        return;
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    if unsafe { GetWindowRect(fenetre, &mut r) } != 0 {
        if let Some(f) = fichier_position() {
            let _ = std::fs::write(f, format!("{} {}", r.left, r.top));
        }
    }
}

/// Replace la fenêtre active à la position mémorisée. Renvoie true si c'est fait,
/// ou s'il n'y a rien à faire (pas de position, ou position hors de tout écran).
#[cfg(windows)]
fn restaurer_position() -> bool {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    let Some(texte) = fichier_position().and_then(|f| std::fs::read_to_string(f).ok()) else { return true };
    let mut n = texte.split_whitespace().filter_map(|v| v.parse::<i32>().ok());
    let (Some(x), Some(y)) = (n.next(), n.next()) else { return true };
    unsafe {
        // Fenêtre active, sinon recherche par son titre exact (au lancement, elle peut ne pas être active).
        let mut fenetre = GetActiveWindow();
        if fenetre.is_null() {
            let titre: Vec<u16> = format!("Moniteur IA — version {VERSION}").encode_utf16().chain([0]).collect();
            fenetre = FindWindowW(std::ptr::null(), titre.as_ptr());
        }
        if fenetre.is_null() {
            return false; // fenêtre pas encore créée : on réessaiera
        }
        // Écran débranché ou résolution changée : si le centre de la barre n'est sur aucun écran,
        // on garde la position par défaut.
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(fenetre, &mut r);
        let centre = POINT { x: x + (r.right - r.left) / 2, y: y + (r.bottom - r.top) / 2 };
        if MonitorFromPoint(centre, MONITOR_DEFAULTTONULL).is_null() {
            return true;
        }
        SetWindowPos(fenetre, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    true
}

#[cfg(not(windows))]
fn restaurer_position() -> bool {
    true
}

/// Bouton en forme de pastille ; renvoie true au clic.
fn pastille(ui: &egui::Ui, droite: f32, y: f32, texte: &str, actif: bool, id: &str) -> (bool, f32) {
    pastille_coloree(ui, droite, y, texte, actif, None, id)
}

/// Pastille avec fond coloré facultatif (mode en cours) : `fond` = (couleur du fond, couleur du texte).
fn pastille_coloree(ui: &egui::Ui, droite: f32, y: f32, texte: &str, actif: bool, fond: Option<(Color32, Color32)>, id: &str) -> (bool, f32) {
    let p = ui.painter();
    let couleur_texte = fond.map_or(TEXTE, |(_, t)| t);
    let galet = p.layout_no_wrap(texte.to_string(), f(13.0), couleur_texte);
    let l = galet.size().x + 24.0;
    let r = Rect::from_min_size(pos2(droite - l, y), vec2(l, 26.0));
    let rep = ui.interact(r, Id::new(id), Sense::click());
    let bord = if actif { ROSE } else if rep.hovered() { DOUX } else { Color32::from_gray(0x44) };
    if let Some((c, _)) = fond {
        p.rect_filled(r, CornerRadius::same(13), c);
    } else if rep.hovered() {
        p.rect_filled(r, CornerRadius::same(13), Color32::from_gray(0x24));
    }
    p.rect_stroke(r, CornerRadius::same(13), Stroke::new(1.0_f32, bord), StrokeKind::Inside);
    p.galley(pos2(r.left() + 12.0, r.center().y - galet.size().y / 2.0), galet, couleur_texte);
    (rep.clicked(), l)
}

fn anneau(p: &egui::Painter, c: Pos2, r: f32, pct: f32, grand: &str, legende: &str) {
    p.circle_stroke(c, r, Stroke::new(3.0_f32, TRAIT));
    let pct = pct.clamp(0.0, 100.0);
    if pct > 0.2 {
        let n = (pct * 1.8).max(3.0) as usize;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * pct / 100.0 * i as f32 / n as f32;
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        let (haut, diam) = (c.y - r, 2.0 * r);
        p.line(pts.clone(), PathStroke::new_uv(5.0_f32, move |_, q| degrade((q.y - haut) / diam)));
        for q in [pts[0], *pts.last().unwrap()] {
            p.circle_filled(q, 2.5, degrade((q.y - haut) / diam));
        }
    }
    p.text(c + vec2(0.0, -6.0), Align2::CENTER_CENTER, grand, g(r * 0.5), Color32::WHITE);
    p.text(c + vec2(0.0, r * 0.36), Align2::CENTER_CENTER, legende, f(13.0), Color32::from_gray(0xdd));
}

struct Bas<'a> {
    gauche: (String, Color32, &'a str),
    droite: (String, &'a str),
}

fn carte(p: &egui::Painter, r: Rect, titre: &str, pct: f32, grand: &str, legende: &str, bas: Bas) {
    p.rect_filled(r, CornerRadius::same(6), CARTE);
    p.text(r.min + vec2(16.0, 14.0), Align2::LEFT_TOP, titre, g(17.0), TEXTE);
    let place_bas = 62.0;
    let dispo = r.height() - 44.0 - place_bas;
    let rayon = (dispo / 2.0 - 6.0).min(r.width() / 2.0 - 28.0).max(30.0);
    let c = pos2(r.center().x, r.top() + 44.0 + dispo / 2.0);
    anneau(p, c, rayon, pct, grand, legende);
    let yb = r.bottom() - 16.0;
    let (gv, gc, gs) = &bas.gauche;
    p.text(pos2(r.left() + 16.0, yb - 18.0), Align2::LEFT_BOTTOM, gv, g(24.0), *gc);
    p.text(pos2(r.left() + 16.0, yb), Align2::LEFT_BOTTOM, *gs, f(12.5), DOUX);
    let (dv, ds) = &bas.droite;
    p.text(pos2(r.right() - 16.0, yb - 18.0), Align2::RIGHT_BOTTOM, dv, g(16.0), TEXTE);
    p.text(pos2(r.right() - 16.0, yb), Align2::RIGHT_BOTTOM, *ds, f(12.5), DOUX);
}

fn ligne_outil(p: &egui::Painter, r: Rect, nom: &str, o: &Outil, separateur: bool) {
    if separateur {
        p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, Color32::from_gray(0x33)));
    }
    let cy = r.center().y;
    let couleur = match o.niveau {
        Niveau::Actif => VERT,
        Niveau::Alerte => ORANGE,
        Niveau::Inactif => Color32::from_gray(0x9a),
        Niveau::Arrete => Color32::from_gray(0x55),
    };
    if o.niveau == Niveau::Actif {
        p.circle_filled(pos2(r.left() + 10.0, cy), 9.0, couleur.gamma_multiply(0.25));
    }
    p.circle_filled(pos2(r.left() + 10.0, cy), 6.0, couleur);
    p.text(pos2(r.left() + 30.0, cy), Align2::LEFT_CENTER, nom, g(14.5), TEXTE);
    let largeur_etat = (r.width() * 0.26).max(150.0);
    let barre = Rect::from_min_max(pos2(r.left() + 130.0, cy - 12.0), pos2(r.right() - largeur_etat - 14.0, cy - 3.0));
    p.rect_filled(barre, CornerRadius::same(5), TRAIT);
    if let Some(pr) = o.progression {
        let mut plein = barre;
        plein.set_width(barre.width() * pr.clamp(0.0, 1.0));
        let (x0, w) = (barre.left(), barre.width());
        let pts = vec![pos2(plein.left() + 4.0, plein.center().y), pos2((plein.right() - 4.0).max(plein.left() + 4.1), plein.center().y)];
        p.line(pts, PathStroke::new_uv(9.0_f32, move |_, q| degrade((q.x - x0) / w)));
    }
    let detail = tronquer(p, &o.detail, f(12.0), barre.width());
    p.text(pos2(barre.left(), cy + 3.0), Align2::LEFT_TOP, detail, f(12.0), DOUX);
    let couleur_etat = if o.niveau == Niveau::Alerte { ORANGE } else if o.niveau == Niveau::Arrete { DOUX } else { TEXTE };
    let xe = r.right();
    if o.conseil.is_empty() {
        p.text(pos2(xe, cy), Align2::RIGHT_CENTER, &o.etat, f(12.5), couleur_etat);
    } else {
        p.text(pos2(xe, cy - 1.0), Align2::RIGHT_BOTTOM, &o.etat, f(12.5), couleur_etat);
        p.text(pos2(xe, cy + 1.0), Align2::RIGHT_TOP, &o.conseil, f(12.5), ORANGE);
    }
}

fn tronquer(p: &egui::Painter, s: &str, police: FontId, max: f32) -> String {
    if p.layout_no_wrap(s.into(), police.clone(), TEXTE).size().x <= max {
        return s.into();
    }
    let mut t: String = s.into();
    while !t.is_empty() {
        t.pop();
        let essai = format!("{t}…");
        if p.layout_no_wrap(essai.clone(), police.clone(), TEXTE).size().x <= max {
            return essai;
        }
    }
    String::new()
}

impl App {
    fn vue_complete(&mut self, ui: &mut egui::Ui, e: &Etat) {
        let ctx = ui.ctx().clone();
        let tout = ui.max_rect().shrink(14.0);
        let p = ui.painter().clone();

        // Barre supérieure
        p.text(pos2(tout.left(), tout.top() + 13.0), Align2::LEFT_CENTER, "MONITEUR IA", g(16.0), TEXTE);
        let sous = match &self.message {
            Some((m, t)) if t.elapsed() < Duration::from_secs(6) => m.clone(),
            _ => "actualisation 1 s".into(),
        };
        p.text(pos2(tout.left() + 122.0, tout.top() + 13.0), Align2::LEFT_CENTER, sous, f(13.0), DOUX);
        let mut x = tout.right();
        let (clic, l) = pastille(ui, x, tout.top(), "Mode compact", false, "compact");
        if clic {
            self.basculer_compact(&ctx);
        }
        x -= l + 10.0;
        let (clic, l) = pastille(ui, x, tout.top(), "Toujours au premier plan", self.premier_plan, "dessus");
        if clic {
            self.premier_plan = !self.premier_plan;
            self.appliquer_niveau(&ctx);
        }
        x -= l + 10.0;
        if App::scripts_presents() {
            if let Some((_, t)) = self.confirmation {
                if t.elapsed() > Duration::from_secs(4) {
                    self.confirmation = None;
                } else {
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            }
            // Mode en cours :
            // 1. pendant 90 s après un clic, le mode choisi (le temps que Forge démarre ou s'arrête) ;
            // 2. ensuite, l'état réel : Forge en marche → images ; WanGP lancé → vidéo ;
            //    rien de lancé → vidéo si c'est le dernier mode choisi (Forge arrêté, prêt pour WanGP), sinon repos.
            let forge_marche = e.forge.niveau != Niveau::Arrete;
            let wangp_marche = !e.wangp_pids.is_empty() || e.wangp.niveau != Niveau::Arrete;
            let choix = self.dernier_choix.map(|(m, _)| m);
            let mode_en_cours = match self.dernier_choix {
                Some((m, t)) if t.elapsed() < Duration::from_secs(90) => m,
                _ if forge_marche => Mode::Images,
                _ if wangp_marche => Mode::Video,
                _ if choix == Some(Mode::Video) => Mode::Video,
                _ => Mode::Repos,
            };
            for (mode, libelle, id) in [(Mode::Repos, "Mode repos", "repos"), (Mode::Images, "Mode images (Forge)", "images"), (Mode::Video, "Mode vidéo (WanGP)", "video")] {
                let attente = self.confirmation.is_some_and(|(m, _)| m == mode);
                let texte = if attente { "Cliquer encore pour confirmer" } else { libelle };
                let fond = (mode == mode_en_cours && !attente).then(|| match mode {
                    Mode::Repos => (VERT, FOND),
                    Mode::Images => (VIOLET, TEXTE),
                    Mode::Video => (ORANGE, FOND),
                });
                let (clic, l) = pastille_coloree(ui, x, tout.top(), texte, attente, fond, id);
                if clic {
                    if attente {
                        self.confirmation = None;
                        self.dernier_choix = Some((mode, Instant::now()));
                        self.lancer_mode(mode, e);
                    } else {
                        self.confirmation = Some((mode, Instant::now()));
                    }
                }
                x -= l + 10.0;
            }
        }

        // Cartes du haut
        let haut = tout.top() + 40.0;
        let hauteur_bas = 236.0_f32.min(tout.height() * 0.42);
        let bas_y = tout.bottom() - hauteur_bas;
        let w4 = (tout.width() - 30.0) / 4.0;
        let case = |i: usize| Rect::from_min_size(pos2(tout.left() + i as f32 * (w4 + 10.0), haut), vec2(w4, bas_y - 10.0 - haut));

        match &e.gpu {
            Some(gp) => {
                let t = gp.temp.map(|t| t as f32);
                carte(&p, case(0), "GPU", gp.util as f32, &format!("{}%", gp.util), "Utilisation GPU", Bas {
                    gauche: (t.map_or("—".into(), |t| format!("{t:.0}°C")), t.map_or(DOUX, |t| couleur_temp(t, 75.0, 85.0)), "Température GPU"),
                    droite: (
                        match (gp.puissance_w, gp.limite_w) {
                            (Some(pw), Some(l)) => format!("{pw:.0} / {l:.0} W"),
                            (Some(pw), None) => format!("{pw:.0} W"),
                            _ => "—".into(),
                        },
                        "Puissance",
                    ),
                });
                let pct = if gp.vram_total > 0 { gp.vram_util as f32 * 100.0 / gp.vram_total as f32 } else { 0.0 };
                let libre = gp.vram_total.saturating_sub(gp.vram_util);
                carte(&p, case(1), "VRAM", pct, &format!("{pct:.0}%"), &format!("{} / {} Go", go(gp.vram_util), go(gp.vram_total)), Bas {
                    gauche: (format!("{} Go libres", go(libre)), if libre < 1_073_741_824 { ORANGE } else { VERT }, "Mémoire vidéo dédiée"),
                    droite: (gp.horloge_mhz.map_or("—".into(), |m| format!("{m} MHz")), "Fréquence GPU"),
                });
            }
            None => {
                for i in 0..2 {
                    let r = case(i);
                    p.rect_filled(r, CornerRadius::same(6), CARTE);
                    p.text(r.min + vec2(16.0, 14.0), Align2::LEFT_TOP, if i == 0 { "GPU" } else { "VRAM" }, g(17.0), TEXTE);
                    let msg = if e.pret { e.gpu_erreur.as_str() } else { "Lecture…" };
                    p.text(r.center(), Align2::CENTER_CENTER, tronquer(&p, msg, f(13.0), r.width() - 24.0), f(13.0), DOUX);
                }
            }
        }
        carte(&p, case(2), "CPU", e.cpu, &format!("{:.0}%", e.cpu), "Utilisation CPU", Bas {
            gauche: (
                e.cpu_temp.map_or("— °C".into(), |t| format!("{t:.0}°C")),
                e.cpu_temp.map_or(DOUX, |t| couleur_temp(t, 80.0, 92.0)),
                if e.cpu_temp.is_some() { "Temp. CPU (zone ACPI)" } else { "Température CPU" },
            ),
            droite: (format!("{} threads", e.threads), ""),
        });
        let pct_ram = if e.ram_total > 0 { e.ram_util as f32 * 100.0 / e.ram_total as f32 } else { 0.0 };
        carte(&p, case(3), "RAM", pct_ram, &format!("{pct_ram:.0}%"), &format!("{} / {} Go", go(e.ram_util), go(e.ram_total)), Bas {
            gauche: (
                e.echange_util.map_or("—".into(), |o| format!("{} Go", go(o))),
                match e.echange_util {
                    Some(o) if o > 4 * 1_073_741_824 => ROUGE,
                    Some(o) if o > 1_073_741_824 => ORANGE,
                    Some(_) => VERT,
                    None => DOUX,
                },
                "Fichier d'échange utilisé",
            ),
            droite: (e.docker_ram.map_or("—".into(), |o| format!("{} Go", go(o))), "Docker / WSL"),
        });

        // Activité IA
        let gauche = Rect::from_min_max(pos2(tout.left(), bas_y), pos2(tout.left() + tout.width() * 0.55 - 5.0, tout.bottom()));
        p.rect_filled(gauche, CornerRadius::same(6), CARTE);
        p.text(gauche.min + vec2(16.0, 14.0), Align2::LEFT_TOP, "ACTIVITÉ IA", g(17.0), TEXTE);
        let zone = Rect::from_min_max(gauche.min + vec2(16.0, 46.0), gauche.max - vec2(16.0, 8.0));
        let h = zone.height() / 3.0;
        for (i, (nom, o)) in [("WanGP", &e.wangp), ("Forge Neo", &e.forge), ("LM Studio", &e.lmstudio)].iter().enumerate() {
            let r = Rect::from_min_size(pos2(zone.left(), zone.top() + i as f32 * h), vec2(zone.width(), h));
            ligne_outil(&p, r, nom, o, i > 0);
        }

        // Processus
        let droite = Rect::from_min_max(pos2(gauche.right() + 10.0, bas_y), tout.max);
        p.rect_filled(droite, CornerRadius::same(6), CARTE);
        p.text(droite.min + vec2(16.0, 14.0), Align2::LEFT_TOP, "PROCESSUS TOP", g(17.0), TEXTE);
        let x0 = droite.left() + 16.0;
        let wcol = (droite.width() - 32.0) * 0.13;
        let cols = [droite.right() - 16.0 - 2.0 * wcol, droite.right() - 16.0 - wcol, droite.right() - 16.0];
        let y = droite.top() + 54.0;
        p.text(pos2(x0, y), Align2::LEFT_CENTER, "Nom", g(12.5), DOUX);
        for (c, t) in cols.iter().zip(["CPU", "VRAM", "RAM"]) {
            p.text(pos2(*c, y), Align2::RIGHT_CENTER, t, g(12.5), DOUX);
        }
        let pas = ((droite.bottom() - y - 12.0) / 5.0).min(30.0);
        for (i, pr) in e.processus.iter().enumerate() {
            let yy = y + 12.0 + pas * i as f32;
            p.line_segment([pos2(x0, yy), pos2(droite.right() - 16.0, yy)], Stroke::new(1.0_f32, Color32::from_gray(0x33)));
            let cy = yy + pas / 2.0;
            p.text(pos2(x0, cy), Align2::LEFT_CENTER, tronquer(&p, &pr.nom, f(13.0), cols[0] - x0 - 50.0), f(13.0), TEXTE);
            let vals = [
                format!("{:.0} %", pr.cpu),
                pr.vram.filter(|v| *v > 0).map_or("—".into(), |v| format!("{} Go", go(v))),
                format!("{} Go", go(pr.ram)),
            ];
            for (c, v) in cols.iter().zip(vals) {
                p.text(pos2(*c, cy), Align2::RIGHT_CENTER, v, f(13.0), TEXTE);
            }
        }
    }

    fn vue_compacte(&mut self, ui: &mut egui::Ui, e: &Etat) {
        let ctx = ui.ctx().clone();
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let fond = ui.interact(r, Id::new("glisser"), Sense::click_and_drag());
        if fond.drag_started() {
            // Le moteur logiciel ignore ViewportCommand::StartDrag : on confie le déplacement à Windows.
            deplacer_fenetre(&ctx);
        }
        if fond.double_clicked() {
            self.basculer_compact(&ctx);
            return;
        }
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(0x1c, 0x1c, 0x1c));
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0_f32, Color32::from_gray(0x33)), StrokeKind::Inside);
        let mut x = r.left() + 12.0;
        let cy = r.center().y;
        // Chaque valeur occupe une case de largeur fixe (celle d'un gabarit maximal), pour que
        // la barre ne change pas de largeur quand un chiffre passe de 9 à 10.
        let mut morceau = |lib: &str, val: String, gabarit: &str, c: Color32| {
            if !lib.is_empty() {
                let gl = p.layout_no_wrap(lib.into(), f(13.0), DOUX);
                let w = gl.size().x;
                p.galley(pos2(x, cy - gl.size().y / 2.0), gl, DOUX);
                x += w + 5.0;
            }
            let case = p.layout_no_wrap(gabarit.into(), g(14.0), c).size().x;
            let gl = p.layout_no_wrap(val, g(14.0), c);
            let w = gl.size().x.max(case);
            p.galley(pos2(x, cy - gl.size().y / 2.0), gl, c);
            x += w + 16.0;
        };
        if let Some(gp) = &e.gpu {
            morceau("GPU", format!("{} %", gp.util), "100 %", TEXTE);
            if let Some(t) = gp.temp {
                morceau("", format!("{t}°"), "100°", couleur_temp(t as f32, 75.0, 85.0));
            }
            morceau("VRAM", format!("{}/{}", go(gp.vram_util), go(gp.vram_total)), "88,8/88,8", TEXTE);
        }
        morceau("RAM", format!("{}/{}", go(e.ram_util), go(e.ram_total)), "88,8/88,8", TEXTE);
        morceau("CPU", format!("{:.0} %", e.cpu), "100 %", TEXTE);
        let actif = [("WanGP", &e.wangp), ("Forge", &e.forge), ("LM", &e.lmstudio)]
            .into_iter()
            .find(|(_, o)| o.niveau == Niveau::Actif);
        if let Some((nom, o)) = actif {
            let v = o.progression.map_or("actif".into(), |pr| format!("{:.0} %", pr * 100.0));
            morceau(nom, v, "100 %", VERT);
        }
        // Largeur au strict nécessaire : contenu + marge de 10 pt + deux boutons (58 pt depuis le bord droit).
        // Le coin haut gauche ne bouge pas (compatible avec la mémoire de position).
        let besoin = (x - 16.0 - r.left() + 10.0 + 58.0).ceil();
        if besoin != self.largeur_compacte {
            self.largeur_compacte = besoin;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(besoin, TAILLE_COMPACTE.y)));
        }
        // Boutons agrandir / fermer
        for (i, (sym, id)) in [("✕", "fermer"), ("", "agrandir")].iter().enumerate() {
            let b = Rect::from_center_size(pos2(r.right() - 18.0 - i as f32 * 28.0, cy), vec2(24.0, 24.0));
            let rep = ui.interact(b, Id::new(*id), Sense::click());
            if rep.hovered() {
                p.rect_filled(b, CornerRadius::same(4), Color32::from_gray(0x33));
            }
            let _ = sym;
            if i == 0 {
                // croix dessinée (indépendante des polices)
                let c = b.center();
                let d = 4.5;
                p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], Stroke::new(1.5_f32, TEXTE));
                p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], Stroke::new(1.5_f32, TEXTE));
            } else {
                // icône « agrandir » dessinée (indépendante des polices)
                let c = Rect::from_center_size(b.center(), vec2(11.0, 9.0));
                p.rect_stroke(c, CornerRadius::same(1), Stroke::new(1.5_f32, TEXTE), StrokeKind::Inside);
            }
            if rep.clicked() {
                if i == 0 {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                } else {
                    self.basculer_compact(&ctx);
                }
            }
        }
    }
}

impl egui_software_backend::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _rendu: &mut egui_software_backend::SoftwareBackend) {
        let e = self.etat.lock().unwrap().clone();
        if std::mem::take(&mut self.compact_au_demarrage) {
            self.basculer_compact(ui.ctx());
        }
        // Replace la barre compacte à sa dernière position, une fois la fenêtre redimensionnée
        // (après 0,3 s) et active ; on abandonne au bout de 5 s.
        if let Some(t) = self.position_a_restaurer {
            if !self.compact || t.elapsed() > Duration::from_secs(5) {
                self.position_a_restaurer = None;
            } else if t.elapsed() > Duration::from_millis(300) && restaurer_position() {
                self.position_a_restaurer = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        }
        ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, FOND);
        if self.compact {
            self.vue_compacte(ui, &e);
        } else {
            self.vue_complete(ui, &e);
        }
    }
}

fn icone() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let c = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let i = (y * n + x) * 4;
            if d < 31.0 {
                rgba[i..i + 4].copy_from_slice(&[0x18, 0x18, 0x18, 255]);
            }
            let ep = (d - 22.0).abs();
            if ep < 5.5 {
                let col = degrade(y as f32 / n as f32);
                let a = ((5.5 - ep).min(1.0) * 255.0) as u8;
                let a = a.max(rgba[i + 3]);
                rgba[i..i + 4].copy_from_slice(&[col.r(), col.g(), col.b(), a]);
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

/// « moniteur-ia.exe --diagnostic » : écrit un relevé texte dans diagnostic.txt, à côté du .exe.
fn diagnostic() {
    use std::fmt::Write;
    let mut t = format!("Moniteur IA {VERSION} — diagnostic\n\n");
    if let Some(mut r) = mesures::Releveur::nouveau() {
        r.releve();
        std::thread::sleep(Duration::from_millis(1200));
        let e = r.releve();
        match &e.gpu {
            Some(g) => {
                let _ = writeln!(t, "GPU {} : {} %, {:?} °C, {:?} W / {:?} W, {:?} MHz, VRAM {} / {} Go", g.nom, g.util, g.temp, g.puissance_w, g.limite_w, g.horloge_mhz, go(g.vram_util), go(g.vram_total));
            }
            None => {
                let _ = writeln!(t, "GPU : {}", e.gpu_erreur);
            }
        }
        let _ = writeln!(t, "CPU {:.0} % ({} threads), temp. zone {:?}", e.cpu, e.threads, e.cpu_temp);
        let _ = writeln!(t, "RAM {} / {} Go, échange {:?} Go, Docker/WSL {:?} Go", go(e.ram_util), go(e.ram_total), e.echange_util.map(go), e.docker_ram.map(go));
        let _ = writeln!(t, "Durée des relevés : rapide {:.1} ms (chaque seconde), lent {:.1} ms (toutes les 5 s)", e.duree_rapide_ms, e.duree_lente_ms);
        let _ = writeln!(t, "WanGP lancé : {}, GPU global : {:?}, LM Studio lancé : {}", e.wangp_lance, e.wangp_gpu, e.lmstudio_lance);
        let _ = writeln!(t, "\nProcessus top :");
        for p in &e.processus {
            let _ = writeln!(t, "  {} : CPU {:.1} %, VRAM {:?} Go, RAM {} Go", p.nom, p.cpu, p.vram.map(go), go(p.ram));
        }
        let f = ia::forge();
        let _ = writeln!(t, "\nForge Neo : {} | {}", f.etat, f.detail);
        let l = ia::lmstudio(e.lmstudio_lance);
        let _ = writeln!(t, "LM Studio : {} | {}", l.etat, l.detail);
        let w = ia::WanGP::default().lire(e.wangp_lance, e.wangp_gpu);
        let _ = writeln!(t, "WanGP : {} | {}\n", w.etat, w.detail);
    }
    t += &ia::diagnostic_wangp();
    let chemin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("diagnostic.txt")))
        .unwrap_or_else(|| "diagnostic.txt".into());
    let _ = std::fs::write(chemin, t);
}

fn main() {
    if std::env::args().any(|a| a == "--diagnostic") {
        diagnostic();
        return;
    }
    // « moniteur-ia.exe --compact » ouvre directement la barre flottante.
    let compact_au_demarrage = std::env::args().any(|a| a == "--compact");
    // Rendu logiciel (processeur) : aucun pilote graphique n'est chargé.
    let mut config = egui_software_backend::SoftwareBackendAppConfiguration::new();
    // Sans cache : moitié moins de mémoire, et pas plus de calcul puisque tout change chaque seconde.
    config.caching = std::env::var_os("MONITEUR_CACHE").is_some();
    config.viewport_builder = egui::ViewportBuilder::default()
        .with_title(format!("Moniteur IA — version {VERSION}"))
        .with_inner_size(TAILLE_NORMALE)
        .with_min_inner_size(TAILLE_MIN)
        .with_icon(icone());
    let _ = egui_software_backend::run_app_with_software_backend(config, move |ctx| {
        charger_polices(&ctx);
        let etat: Partage = Arc::new(Mutex::new(Etat::default()));
        mesures::demarrer(etat.clone(), ctx.clone());
        App { etat, premier_plan: false, compact: false, confirmation: None, dernier_choix: None, message: None, compact_au_demarrage, position_a_restaurer: None, largeur_compacte: 0.0 }
    });
}
