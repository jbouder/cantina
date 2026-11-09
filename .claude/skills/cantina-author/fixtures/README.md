# Fixtures

Each folder holds a one-line prompt (`prompt.txt`) and the file written for
it by following this skill: read the references, draft, `cantina-cli
check`, `cantina-cli render`, adjust. They are kept so a change to a format
or to `cantina-cli` that breaks an authoring path shows up:
`scripts/skill-fixtures.sh` checks and renders every one; run it by hand
when a format or `cantina-cli` changes (it is not part of CI).

| Prompt | Output |
| --- | --- |
| a fuzz with an octave-up blend | `octave-fuzz/octave-fuzz.pedal.json` |
| a half-time beat at 140 | `half-time-140/half-time-140.pattern.json` |
| a kit for lo-fi hip hop | `lofi-kit/lofi.recipe.json` |

To refresh one, start a new agent session in the repository with the
skill available, give it the prompt, and replace the output with what it
writes once `check` and `render` pass.
