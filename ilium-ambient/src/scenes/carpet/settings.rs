//! Typed, persisted Carpet controls. Numeric values are integers with explicit units.
use crate::{Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};
use super::model::Mode;

pub const MODE_LABELS: &[&str] = &["Mouse hunters", "Autonomous Snake", "Slow Life", "Automated chess", "Lichess TV chess", "DVD ball", "Planetary orbits", "Digital clock", "Analog clock"];
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CarpetSettings {
    pub mode: i32,
    pub yaw: i32,
    pub pitch: i32,
    pub zoom: i32,
    pub hatch_direction: i32,
    pub spacing: i32,
    pub line_width: i32,
    pub height: i32,
    pub radius: i32,
    pub easing_ms: i32,
    pub fps: i32,
    pub simulation_speed: i32,
    pub seed: i32,
    pub hunters_count: i32,
    pub hunters_speed: i32,
    pub hunters_separation: i32,
    pub snake_grid: i32,
    pub snake_step_ms: i32,
    pub snake_initial_length: i32,
    pub snake_food_count: i32,
    pub life_grid: i32,
    pub life_generation_ms: i32,
    pub life_density: i32,
    pub dvd_speed: i32,
    pub orbit_speed: i32,
    pub orbit_scale: i32,
    pub utc_offset_minutes: i32,
    pub chess_move_ms: i32,
    pub chess_easing_ms: i32,
    pub pawn_height: i32,
    pub knight_height: i32,
    pub bishop_height: i32,
    pub rook_height: i32,
    pub queen_height: i32,
    pub king_height: i32,
    pub life_wrap: bool,
    pub clock_seconds: bool,
    pub clock_24h: bool,
    pub clock_tubes: bool,
}
impl Default for CarpetSettings {
 fn default() -> Self { Self {
mode: 0,
yaw: 45,
pitch: 30,
zoom: 100,
hatch_direction: 0,
spacing: 5,
line_width: 65,
height: 100,
radius: 30,
easing_ms: 300,
fps: 20,
simulation_speed: 100,
seed: 17,
hunters_count: 6,
hunters_speed: 20,
hunters_separation: 50,
snake_grid: 16,
snake_step_ms: 200,
snake_initial_length: 4,
snake_food_count: 3,
life_grid: 24,
life_generation_ms: 1500,
life_density: 25,
dvd_speed: 15,
orbit_speed: 100,
orbit_scale: 100,
utc_offset_minutes: 0,
chess_move_ms: 1500,
chess_easing_ms: 650,
pawn_height: 45,
knight_height: 75,
bishop_height: 85,
rook_height: 65,
queen_height: 115,
king_height: 130,
life_wrap: true,
clock_seconds: true,
clock_24h: true,
clock_tubes: true,
} }
}
impl CarpetSettings {
 pub fn mode(&self) -> Mode {
 match self.mode {
0 => Mode::Hunters,
1 => Mode::Snake,
2 => Mode::Life,
3 => Mode::AutoChess,
4 => Mode::LiveChess,
5 => Mode::Dvd,
6 => Mode::Orbits,
7 => Mode::DigitalClock,
8 => Mode::AnalogClock,
_ => Mode::Hunters,
}
}
}
impl SceneSettings for CarpetSettings {
 fn normalized(&self) -> Self { let mut next=self.clone();
next.mode=next.mode.clamp(0,8);
next.yaw=next.yaw.clamp(0,360);
next.pitch=next.pitch.clamp(15,75);
next.zoom=next.zoom.clamp(30,200);
next.hatch_direction=next.hatch_direction.clamp(0,180);
next.spacing=next.spacing.clamp(2,24);
next.line_width=next.line_width.clamp(20,200);
next.height=next.height.clamp(5,250);
next.radius=next.radius.clamp(5,150);
next.easing_ms=next.easing_ms.clamp(0,3000);
next.fps=next.fps.clamp(1,30);
next.simulation_speed=next.simulation_speed.clamp(5,500);
next.seed=next.seed.clamp(0,999999);
next.hunters_count=next.hunters_count.clamp(1,64);
next.hunters_speed=next.hunters_speed.clamp(1,100);
next.hunters_separation=next.hunters_separation.clamp(0,200);
next.snake_grid=next.snake_grid.clamp(4,32);
next.snake_step_ms=next.snake_step_ms.clamp(30,3000);
next.snake_initial_length=next.snake_initial_length.clamp(2,32);
next.snake_food_count=next.snake_food_count.clamp(1,16);
next.life_grid=next.life_grid.clamp(4,32);
next.life_generation_ms=next.life_generation_ms.clamp(100,10000);
next.life_density=next.life_density.clamp(1,90);
next.dvd_speed=next.dvd_speed.clamp(1,100);
next.orbit_speed=next.orbit_speed.clamp(1,500);
next.orbit_scale=next.orbit_scale.clamp(20,150);
next.utc_offset_minutes=next.utc_offset_minutes.clamp(-720,840);
next.chess_move_ms=next.chess_move_ms.clamp(200,10000);
next.chess_easing_ms=next.chess_easing_ms.clamp(0,3000);
next.pawn_height=next.pawn_height.clamp(5,200);
next.knight_height=next.knight_height.clamp(5,200);
next.bishop_height=next.bishop_height.clamp(5,200);
next.rook_height=next.rook_height.clamp(5,200);
next.queen_height=next.queen_height.clamp(5,250);
next.king_height=next.king_height.clamp(5,250);
next.snake_grid -= next.snake_grid % 2;
next
}
 fn controls(&self) -> Vec<Control> {
 let s=self.normalized();
 let mut rows=vec![
Control::choice("carpet_mode","Simulation",s.mode as usize,MODE_LABELS,"Choose one of nine hidden-object simulations."),
Control::slider("carpet_yaw","Camera yaw",s.yaw,(0,360,5),"°","Rotate the ground plane without rotating hidden model coordinates."),
Control::slider("carpet_pitch","Camera pitch",s.pitch,(15,75,5),"°","Ground elevation angle; 30 degrees gives an isometric view."),
Control::slider("carpet_zoom","Ground zoom",s.zoom,(30,200,5),"%","Scale the projected carpet."),
Control::slider("carpet_hatch_direction","Hatch direction",s.hatch_direction,(0,180,5),"°","All lines share this direction in the ground plane."),
Control::slider("carpet_spacing","Hatch spacing",s.spacing,(2,24,1)," dots","Distance between parallel hatch lines measured in Braille dots."),
Control::slider("carpet_line_width","Line width",s.line_width,(20,200,5),"%","Thickness of one hatch line relative to one Braille dot."),
Control::slider("carpet_height","Lift height",s.height,(5,250,5),"%","Scale every hidden object height."),
Control::slider("carpet_radius","Object radius",s.radius,(5,150,5),"‰","Radius in thousandths of the normalized ground width."),
Control::slider("carpet_easing_ms","Motion easing",s.easing_ms,(0,3000,50)," ms","Smooth transition duration for births, moves and clock changes."),
Control::slider("carpet_fps","Frame rate",s.fps,(1,30,1)," fps","Maximum Carpet redraw cadence; lower values reduce rendering work."),
Control::slider("carpet_simulation_speed","Simulation speed",s.simulation_speed,(5,500,5),"%","Scale games and motion; civil clocks always display live time."),
Control::slider("carpet_seed","Seed",s.seed,(0,999999,1),"","Reproducible simulation seed. A change starts a new simulation."),
Control::slider("carpet_hunters_count","Hunters",s.hunters_count,(1,64,1),"","Number of mouse-following hunters."),
Control::slider("carpet_hunters_speed","Hunter speed",s.hunters_speed,(1,100,1),"%/s","Ground width travelled each second."),
Control::slider("carpet_hunters_separation","Separation",s.hunters_separation,(0,200,5),"‰","Minimum flock separation influence."),
Control::slider("carpet_snake_grid","Snake grid",s.snake_grid,(4,32,2)," cells","Even-sized safe Hamiltonian board."),
Control::slider("carpet_snake_step_ms","Snake move",s.snake_step_ms,(30,3000,10)," ms","Interval between moves."),
Control::slider("carpet_snake_initial_length","Initial length",s.snake_initial_length,(2,32,1),"","Snake length at the beginning of each game."),
Control::slider("carpet_snake_food_count","Food count",s.snake_food_count,(1,16,1),"","Maximum simultaneous food targets."),
Control::slider("carpet_life_grid","Life grid",s.life_grid,(4,32,1)," cells","Square Conway Life board."),
Control::slider("carpet_life_generation_ms","Life generation",s.life_generation_ms,(100,10000,100)," ms","Slow default generation interval; easing interpolates births and deaths."),
Control::slider("carpet_life_density","Life population",s.life_density,(1,90,1),"%","Initial seeded live-cell density."),
Control::slider("carpet_dvd_speed","Ball speed",s.dvd_speed,(1,100,1),"%/s","Bouncing ball ground speed."),
Control::slider("carpet_orbit_speed","Orbit speed",s.orbit_speed,(1,500,5),"%","Stylized planetary motion rate; not an astronomical ephemeris."),
Control::slider("carpet_orbit_scale","Orbit scale",s.orbit_scale,(20,150,5),"%","Scale the planetary system within the ground."),
Control::slider("carpet_utc_offset_minutes","UTC offset",s.utc_offset_minutes,(-720,840,15)," min","Civil clock offset from UTC; default explicitly displays UTC."),
Control::slider("carpet_chess_move_ms","Chess move",s.chess_move_ms,(200,10000,100)," ms","Autonomous move interval."),
Control::slider("carpet_chess_easing_ms","Piece easing",s.chess_easing_ms,(0,3000,50)," ms","Duration of eased chess movements and capture fades."),
Control::slider("carpet_pawn_height","Pawn height",s.pawn_height,(5,200,5),"%","Lift for pawn pieces relative to base height."),
Control::slider("carpet_knight_height","Knight height",s.knight_height,(5,200,5),"%","Lift for knight pieces relative to base height."),
Control::slider("carpet_bishop_height","Bishop height",s.bishop_height,(5,200,5),"%","Lift for bishop pieces relative to base height."),
Control::slider("carpet_rook_height","Rook height",s.rook_height,(5,200,5),"%","Lift for rook pieces relative to base height."),
Control::slider("carpet_queen_height","Queen height",s.queen_height,(5,250,5),"%","Lift for queen pieces relative to base height."),
Control::slider("carpet_king_height","King height",s.king_height,(5,250,5),"%","Lift for king pieces relative to base height."),
Control::toggle("carpet_life_wrap","Life wrap",s.life_wrap,"Join opposite Life board edges."),
Control::toggle("carpet_clock_seconds","Clock seconds",s.clock_seconds,"Include seconds in both civil clocks."),
Control::toggle("carpet_clock_24h","24-hour clock",s.clock_24h,"Use 24-hour digital numbers."),
Control::toggle("carpet_clock_tubes","Clock hands",s.clock_tubes,"Draw analog hands as hidden tubes between the center and live points."),
 ];
 rows.retain(|row| match row.id {
 "carpet_hunters_count"|"carpet_hunters_speed"|"carpet_hunters_separation" => s.mode==0,
 "carpet_snake_grid"|"carpet_snake_step_ms"|"carpet_snake_initial_length"|"carpet_snake_food_count" => s.mode==1,
 "carpet_life_grid"|"carpet_life_generation_ms"|"carpet_life_density"|"carpet_life_wrap" => s.mode==2,
 "carpet_chess_move_ms" => s.mode==3,
 "carpet_chess_easing_ms"|"carpet_pawn_height"|"carpet_knight_height"|"carpet_bishop_height"|"carpet_rook_height"|"carpet_queen_height"|"carpet_king_height" => s.mode==3||s.mode==4,
 "carpet_dvd_speed" => s.mode==5,
 "carpet_orbit_speed"|"carpet_orbit_scale" => s.mode==6,
 "carpet_utc_offset_minutes"|"carpet_clock_seconds" => s.mode==7||s.mode==8,
 "carpet_clock_24h" => s.mode==7,
 "carpet_clock_tubes" => s.mode==8,
 _=>true,
 });
 rows
 }
 fn set_control(&mut self,id:&str,value:ControlValue)->Result<bool,String>{
 let before=self.clone();
 match (id,value) {
 ("carpet_mode",ControlValue::Index(index)) if index < MODE_LABELS.len() => self.mode=index as i32,
("carpet_yaw",ControlValue::Number(value)) => self.yaw=value,
("carpet_pitch",ControlValue::Number(value)) => self.pitch=value,
("carpet_zoom",ControlValue::Number(value)) => self.zoom=value,
("carpet_hatch_direction",ControlValue::Number(value)) => self.hatch_direction=value,
("carpet_spacing",ControlValue::Number(value)) => self.spacing=value,
("carpet_line_width",ControlValue::Number(value)) => self.line_width=value,
("carpet_height",ControlValue::Number(value)) => self.height=value,
("carpet_radius",ControlValue::Number(value)) => self.radius=value,
("carpet_easing_ms",ControlValue::Number(value)) => self.easing_ms=value,
("carpet_fps",ControlValue::Number(value)) => self.fps=value,
("carpet_simulation_speed",ControlValue::Number(value)) => self.simulation_speed=value,
("carpet_seed",ControlValue::Number(value)) => self.seed=value,
("carpet_hunters_count",ControlValue::Number(value)) => self.hunters_count=value,
("carpet_hunters_speed",ControlValue::Number(value)) => self.hunters_speed=value,
("carpet_hunters_separation",ControlValue::Number(value)) => self.hunters_separation=value,
("carpet_snake_grid",ControlValue::Number(value)) => self.snake_grid=value,
("carpet_snake_step_ms",ControlValue::Number(value)) => self.snake_step_ms=value,
("carpet_snake_initial_length",ControlValue::Number(value)) => self.snake_initial_length=value,
("carpet_snake_food_count",ControlValue::Number(value)) => self.snake_food_count=value,
("carpet_life_grid",ControlValue::Number(value)) => self.life_grid=value,
("carpet_life_generation_ms",ControlValue::Number(value)) => self.life_generation_ms=value,
("carpet_life_density",ControlValue::Number(value)) => self.life_density=value,
("carpet_dvd_speed",ControlValue::Number(value)) => self.dvd_speed=value,
("carpet_orbit_speed",ControlValue::Number(value)) => self.orbit_speed=value,
("carpet_orbit_scale",ControlValue::Number(value)) => self.orbit_scale=value,
("carpet_utc_offset_minutes",ControlValue::Number(value)) => self.utc_offset_minutes=value,
("carpet_chess_move_ms",ControlValue::Number(value)) => self.chess_move_ms=value,
("carpet_chess_easing_ms",ControlValue::Number(value)) => self.chess_easing_ms=value,
("carpet_pawn_height",ControlValue::Number(value)) => self.pawn_height=value,
("carpet_knight_height",ControlValue::Number(value)) => self.knight_height=value,
("carpet_bishop_height",ControlValue::Number(value)) => self.bishop_height=value,
("carpet_rook_height",ControlValue::Number(value)) => self.rook_height=value,
("carpet_queen_height",ControlValue::Number(value)) => self.queen_height=value,
("carpet_king_height",ControlValue::Number(value)) => self.king_height=value,
("carpet_life_wrap",ControlValue::Bool(value)) => self.life_wrap=value,
("carpet_clock_seconds",ControlValue::Bool(value)) => self.clock_seconds=value,
("carpet_clock_24h",ControlValue::Bool(value)) => self.clock_24h=value,
("carpet_clock_tubes",ControlValue::Bool(value)) => self.clock_tubes=value,
 (id,_) if id.starts_with("carpet_") => return Err("Invalid Carpet control or value".into()),
 _=>return Ok(false),
 }
 *self=self.normalized();
 Ok(*self!=before)
 }
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn every_mode_has_unique_valid_editable_controls(){
 for mode in 0..9 {let mut s=CarpetSettings {mode,..Default::default()};
 let rows=s.controls();let mut ids=std::collections::HashSet::new();
 for row in rows {assert!(ids.insert(row.id)); if let Some(value)=row.stepped(1){assert!(s.set_control(row.id,value).is_ok());}}
 assert_eq!(s,s.normalized());
 }
 }
 #[test] fn wrong_types_preserve_settings_and_unknown_controls_are_ignored(){
 let mut s=CarpetSettings::default();let before=s.clone();
 assert!(s.set_control("carpet_spacing",ControlValue::Bool(true)).is_err());assert_eq!(s,before);
 assert!(!s.set_control("foreign",ControlValue::Number(1)).unwrap());
 }
 #[test] fn saved_all_mode_controls_roundtrip_and_extremes_normalize(){
 let s=CarpetSettings {mode:8,spacing:-99,snake_grid:31,utc_offset_minutes:9999,..Default::default()}.normalized();
 assert_eq!(s.spacing,2);assert_eq!(s.snake_grid,30);assert_eq!(s.utc_offset_minutes,840);
 let json=serde_json::to_string(&s).unwrap();assert_eq!(serde_json::from_str::<CarpetSettings>(&json).unwrap(),s);
 }
}
