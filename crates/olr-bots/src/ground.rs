//! What a bot's probes read (issue #91): the level's ground alone, or the
//! world's one terrain query ([`Solid`]: the ground and the live rock parts
//! where they are this tick). No OLX counterpart: OLX's rock never moves.
//!
//! Every probe is generic over [`Ground`]; a bot's `think` reads
//! `World::solid()`, so it sees a part as rock where it is. The navigation
//! cache ([`crate::nav::AreaCache`]) reads [`Ground::ground`] alone: shared
//! and append-only, it never depends on where a part was when it grew.

use olr_sim::Solid;
use olr_terrain::Terrain;

/// A pixel's flags as a bot reads them.
pub trait Ground {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    /// OLX's pixel flags ([`Terrain::flags`]; outside the map, rock).
    fn flags(&self, x: i64, y: i64) -> u8;
    /// The level's ground under any live part.
    fn ground(&self) -> &Terrain;
}

impl Ground for Terrain {
    #[inline(always)]
    fn width(&self) -> u32 {
        Terrain::width(self)
    }

    #[inline(always)]
    fn height(&self) -> u32 {
        Terrain::height(self)
    }

    #[inline(always)]
    fn flags(&self, x: i64, y: i64) -> u8 {
        Terrain::flags(self, x, y)
    }

    fn ground(&self) -> &Terrain {
        self
    }
}

impl Ground for Solid<'_> {
    #[inline(always)]
    fn width(&self) -> u32 {
        Solid::width(self)
    }

    #[inline(always)]
    fn height(&self) -> u32 {
        Solid::height(self)
    }

    #[inline(always)]
    fn flags(&self, x: i64, y: i64) -> u8 {
        Solid::flags(self, x, y)
    }

    fn ground(&self) -> &Terrain {
        self.terrain
    }
}
