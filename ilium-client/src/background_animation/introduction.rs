//! Two-sentence introductions shown at the top of each animation's settings.
//! Kept apart from `AnimationKind::description`, which stays the one-line
//! scene-list help.

use super::AnimationKind;

impl AnimationKind {
    /// What this animation is and what it is about, in two sentences.
    pub fn introduction(self) -> &'static str {
        match self {
            Self::Video => "Plays video files, folders or URLs as dithered Braille. It turns any moving picture into a text-mode image on your screen.",
            Self::Spectrum => "A spectrum analyzer draws whatever your system is playing as bars of frequency. It shows the sound of your music or audio in real time.",
            Self::Images => "Colored Braille images come from files, folders or URLs, with a slow pan and zoom. It turns a photo collection into a gentle, moving slideshow.",
            Self::PluginScene => "A scene supplied by an enabled plugin, such as the dithered waves of the Scenes plugin. Pick the scene here and set its own options; colour, dithering and display use the shared rows.",
            Self::Wikipedia => "Today's Wikipedia articles scroll slowly as readable text or as font-rendered Braille, with images and infoboxes. It turns the day's encyclopedia into a background you can read.",
            Self::OpenStreetMap => "Real OpenStreetMap streets, buildings, waterways, parks and railways are drawn around ten world places. Braille dots show fixed or panning cameras, so you can explore the map.",
            Self::Semantic => "This shows the animation recommended during tree reorganization for the selected project or entry. It follows the structure of your work rather than a fixed choice.",
        }
    }
}
