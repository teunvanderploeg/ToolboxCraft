#[derive(Debug, Clone, Copy)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}
impl App {
    pub fn repo(&self) -> String {
        format!("storytold/{}", self.id)
    }
    pub fn bundle_id(&self) -> String {
        format!("ai.storyteller.{}", self.id)
    }
    pub fn website(&self) -> String {
        format!("https://getartcraft.com/apps/{}", self.id)
    }
}
macro_rules! app {
    ($id:literal, $name:literal, $desc:literal) => {
        App {
            id: $id,
            name: $name,
            description: $desc,
        }
    };
}
pub const CATALOG: [App; 12] = [
    app!("photocraft", "PhotoCraft", "Image editing"),
    app!("vectorcraft", "VectorCraft", "Vector illustration"),
    app!("filmcraft", "FilmCraft", "Video editing"),
    app!(
        "lightcraft",
        "LightCraft",
        "Photo library & RAW development"
    ),
    app!("pdfcraft", "PdfCraft", "PDF workbench"),
    app!("effectcraft", "EffectCraft", "Motion graphics & VFX"),
    app!("designcraft", "DesignCraft", "Page layout & publishing"),
    app!("wordcraft", "WordCraft", "Word processing"),
    app!("soundcraft", "SoundCraft", "Audio recording & editing"),
    app!("deckcraft", "DeckCraft", "Presentations"),
    app!("cadcraft", "CADCraft", "Computer-aided design"),
    app!("gridcraft", "GridCraft", "Spreadsheets & data analysis"),
];
pub fn by_id(id: &str) -> Option<App> {
    CATALOG.iter().find(|a| a.id == id).copied()
}
