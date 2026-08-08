// Known false positive, not a bug: the original ESLint rule flagged every
// identifier named Map, including member names, so mapboxgl.Map cannot be
// told apart from a bare global Map. See docs/RULES.md.
const map = new mapboxgl.Map({ container: 'map' });

// The parameter's own declaration is never flagged -- declaring a local
// named Map isn't itself a use of a value -- but the semantic model
// correctly resolves `new Map()` to the parameter (there is no Immutable
// import anywhere in this file for it to shadow), so it's still native Map
// and still flagged, exactly as it would be without the parameter at all.
function useShadowedMap(Map) {
  return new Map();
}
