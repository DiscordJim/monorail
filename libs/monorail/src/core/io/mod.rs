use crate::core::io::ring::IoRingDriver;

pub mod fs;
pub mod ring;
pub mod net;


pub trait FromRing<'a>: 'a {
    fn from_ring(driver: &'a IoRingDriver) -> Self;
}