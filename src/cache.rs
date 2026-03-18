/// Single source of truth for all cache format versions.
/// Bump this when ANY cache format changes — all caches invalidate together,
/// which is safe because rebuilds are fast and correct.
pub const CACHE_VERSION: u32 = 3;
