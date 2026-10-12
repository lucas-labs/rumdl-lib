//! GitHub.com official anchor generation with security hardening
//!
//! This module implements the exact anchor generation algorithm used by GitHub.com,
//! verified through comprehensive testing with GitHub Gists, with comprehensive
//! security hardening against injection attacks and DoS vectors.
//!
//! Algorithm verified against GitHub.com (not third-party packages):
//! 1. Input validation and size limits (max 10KB)
//! 2. Unicode normalization (NFC) to prevent homograph attacks
//! 3. Dangerous Unicode filtering (RTL override, zero-width, control chars)
//! 4. Lowercase conversion
//! 5. HTML comment, markdown formatting (*, `, []) and HTML tag removal with ReDoS-safe patterns
//! 6. Multi-character pattern replacement (-->, <->, ==>, ->)
//! 7. Em/en dash removal and special symbol replacement (& → --, © → --)
//! 8. Character processing: letters, digits, underscores and hyphens are kept, each space
//!    becomes a hyphen, and every other symbol (emoji included) is deleted, so the spaces
//!    around an emoji stay
//! 9. No leading/trailing trimming (unlike kramdown)
//!
//! Security measures implemented:
//! - Input size limits to prevent memory exhaustion
//! - Unicode normalization to prevent homograph attacks
//! - Bidirectional text injection prevention
//! - Zero-width character stripping
//! - Control character filtering
//! - ReDoS-resistant regex patterns with complexity limits
