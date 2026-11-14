use crate::{
    Result,
    protocol::params::{HEIGHT, WIDTH},
    state::State,
};
use std::{io::Write, path::Path};
pub fn ppm(state: &State, path: &Path) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{WIDTH} {HEIGHT}\n255\n")?;
    for parcel in &state.parcels {
        file.write_all(&parcel.color.0)?;
    }
    Ok(())
}
