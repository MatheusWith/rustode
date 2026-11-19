# Provenance sidecar

Use this sidecar for any extracted, generated or captured file that goes into `09-findings/` or `docs/`. Examples: a snippet extracted from the asar, a new screenshot, a tool output. Save it next to the file as `<file>.provenance`.

```text
file:         <file name>
what-it-is:   <what it shows or contains>
origin:       <path in the source tree @ commit | path inside the asar | test run | app capture>
sha256:       <the file's hash>
tool:         <tool and version, exact command>
license:      <MIT (VS Code/VSCodium) | the license of the source module | owner's decision>
added:        YYYY-MM-DD by <who>
notes:        <how to reproduce>
```
