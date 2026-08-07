// No Immutable import in this file at all, so nothing here is suppressed by
// the file-level "Immutable's Map is bound" state.

// Known false positive, not a bug: the original ESLint rule flagged every
// identifier named Map, including member names, so mapboxgl.Map cannot be
// told apart from a bare global Map. See docs/RULES.md.
const map = new mapboxgl.Map({ container: 'map' });

// No real scope analysis: a parameter named Map shadowing the global is
// still flagged, both at its declaration and at its use, exactly like the
// original ESLint rule (which had the same limitation).
function useShadowedMap(Map) {
  return new Map();
}
