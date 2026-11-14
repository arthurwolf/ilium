//! Two-sentence introductions shown at the top of each animation's settings.
//! Kept apart from `AnimationKind::description`, which stays the one-line
//! scene-list help.

use super::AnimationKind;

impl AnimationKind {
    /// What this animation is and what it is about, in two sentences.
    pub fn introduction(self) -> &'static str {
        match self {
            Self::Shoreline => "A diagonal wash of water runs up a beach, leaving fine foam, wet sand and scattered grains. It is a calm study of the moment where waves meet land.",
            Self::MoonlitWater => "Crossing wavelets break up the reflection of a moon on the sea, which widens and narrows as the water moves. It shows how light behaves on a restless surface at night.",
            Self::SleepingRidge => "Layered clouds and valley mist drift slowly over quiet, ridged hills. It is a still landscape in which only the weather moves.",
            Self::WindyHillside => "A hillside of fine grass and seed heads bends in traveling gusts of wind. It shows how a whole meadow responds to a single breeze.",
            Self::TeaSteam => "Thin translucent wisps of steam curl up from a rounded porcelain cup. It is a small, quiet still life in motion.",
            Self::Kelp => "Clusters of tapering ribbons sway and twist through layered underwater currents. It mimics a kelp forest moving with the flow of the sea.",
            Self::StoneCaustics => "A web of moving light bends over smooth stones, pebbles and sparse sand. It recreates the bright, shifting pattern that sunlight casts on the bottom of a shallow pool.",
            Self::Cloudlets => "Soft islands of cloud gather, join and separate as they drift across the sky. It shows how small weather systems form and dissolve.",
            Self::TwoRipples => "Two wave sources send outward crests that brighten and dim where their fields meet. It is a simple view of interference, the way two waves combine.",
            Self::QuietPond => "Notched lily pads rest on a pond while faint reflections drift beneath the surface. It is a slow, peaceful view of still water.",
            Self::Stars => "This shows the real night sky above your location, right now, as a star map. It lets you look up at the stars without leaving your desk.",
            Self::NightLights => "City lights seen from orbit are drawn on a borderless map of the dark Earth. The glowing clusters show where people live on the night side of the planet.",
            Self::Clouds => "Live weather-satellite imagery shows clouds across the globe or over your location. It keeps the sky on screen in step with the real one.",
            Self::Video => "Plays video files, folders or URLs as dithered Braille. It turns any moving picture into a text-mode image on your screen.",
            Self::Spectrum => "A spectrum analyzer draws whatever your system is playing as bars of frequency. It shows the sound of your music or audio in real time.",
            Self::Images => "Colored Braille images come from files, folders or URLs, with a slow pan and zoom. It turns a photo collection into a gentle, moving slideshow.",
            Self::PluginScene => "A scene supplied by an enabled plugin, such as the dithered waves of the Scenes plugin. Pick the scene here and set its own options; colour, dithering and display use the shared rows.",
            Self::VectorTd => "A tower defense game plays itself, with an AI building, upgrading and unlocking glowing vector towers. It fights waves of monsters, level after level, across several maps.",
            Self::Wikipedia => "Today's Wikipedia articles scroll slowly as readable text or as font-rendered Braille, with images and infoboxes. It turns the day's encyclopedia into a background you can read.",
            Self::VoxelLandscape => "A seeded isometric block world of forests, deserts, villages, caves and ravines drifts past. It can be shown in monochrome or pastel dithering.",
            Self::TopographicMaps => "Contour maps of Earth, the Moon, Mars, Venus, Mercury, Ceres and fictional worlds are drawn from real elevation surveys. They pan slowly across a flat map or turn as a globe, both drawn in Braille dots.",
            Self::Graph => "Public observations are shown as Braille lines, bars or OHLC candles, from selectable sources and time scales. It is a live chart of the data you choose.",
            Self::Earthquakes => "USGS earthquake events of every reported magnitude appear on a coastline map. Pulsing markers and magnitude labels show where the ground has recently moved.",
            Self::Aircraft => "OpenSky's reported airborne positions appear worldwide on a map, updated anonymously every fifteen minutes. It shows the air traffic overhead, as it is reported.",
            Self::Boats => "Received AIS ship positions are drawn on a world coastline, from OpenSeaFeed or Finnish Digitraffic. Coverage is incomplete, so not every vessel at sea appears.",
            Self::Chess => "The featured Lichess TV game is shown as its actual positions, with dithered piece silhouettes. It follows a live game as it is played.",
            Self::OpenStreetMap => "Real OpenStreetMap streets, buildings, waterways, parks and railways are drawn around ten world places. Braille dots show fixed or panning cameras, so you can explore the map.",
            Self::Carpet => "Isometric hatch lines lift over hidden moving spheres and tubes that play several small games and simulations. Among them are mouse hunters, Snake, Life, chess, a DVD ball, planets and clocks.",
            Self::Wind => "Dots are blown across the empty parts of your screen by a fixed or rotating wind. Scrolling and new text push them around.",
            Self::Growth => "A fungal colony grows from seeded points and branches across the screen. The pointer and foreground text clear parts of it as it spreads.",
            Self::Semantic => "This shows the animation recommended during tree reorganization for the selected project or entry. It follows the structure of your work rather than a fixed choice.",
            Self::Frost => "Fine branching ice grows and retreats around the screen edges or a foreground character mask. It frosts the border of your view.",
            Self::Embroidery => "A moving stitch progressively reveals a delicate geometric flower. It looks like a needle slowly completing a pattern.",
            Self::HesitatingInk => "A gently curling stroke pauses, resumes, fades and begins again. It is a line of ink that moves with hesitation.",
        }
    }
}
