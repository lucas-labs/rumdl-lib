//! Pure kramdown anchor generation with security and performance hardening
//!
//! This module implements the exact anchor generation algorithm used by pure
//! kramdown (without GFM input), verified against the official kramdown Ruby gem,
//! with additional security hardening and performance optimizations.
//!
//! Algorithm verified against kramdown 2.5.1 Ruby gem:
//! 1. Input validation and size limits
//! 2. Unicode normalization and security filtering
//! 3. Character filtering (ASCII letters, numbers, spaces, hyphens only)
//! 4. Symbol replacements (arrows, ampersands with spaces)
//! 5. Leading character removal until first letter
//! 6. Space → hyphen, case conversion
//! 7. Leading hyphen removal, preserve trailing
//!
//! Key differences from Jekyll/GFM:
//! - Removes underscores entirely
//! - More aggressive character filtering
//! - Preserves all hyphens (no consolidation)
//!
//! Security features:
//! - Unicode normalization (NFC)
//! - Control character filtering
//! - Zero-width character removal
//! - RTL/bidirectional character filtering
//! - Input size limits (prevents DoS)
//! - Performance bounds on consecutive patterns
