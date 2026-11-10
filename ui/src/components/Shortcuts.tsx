import { shortcutSections } from "../lib/shortcuts";
import { Sheet } from "./Sheet";

type Props = {
  onClose: () => void;
};

/**
 * Help ▸ Keyboard Shortcuts: every shortcut, by menu and by part of the
 * window, from the action registry, the same list as the docs page.
 */
export function Shortcuts({ onClose }: Props) {
  const sections = shortcutSections();
  return (
    <Sheet title="Keyboard shortcuts" onClose={onClose} wide>
      <p className="m-0 text-ink-dim text-body">
        Keys without ⌘ do nothing while you are typing in a text field.
      </p>
      <div className="grid gap-x-6 gap-y-4 sm:grid-cols-2">
        {sections.map((section) => (
          <section key={section.title} aria-labelledby={`keys-${section.title}`}>
            <h3 id={`keys-${section.title}`} className="caption m-0 mb-1">
              {section.title}
              {section.note ? (
                <small className="hint block text-ink-dim">{section.note}</small>
              ) : null}
            </h3>
            <table className="w-full border-collapse text-body">
              <tbody>
                {section.rows.map((row) => (
                  <tr key={`${row.keys}-${row.does}`}>
                    <th
                      scope="row"
                      className="w-28 py-0.5 pr-3 text-left align-top font-lcd font-normal"
                    >
                      {row.keys}
                    </th>
                    <td className="py-0.5 align-top">{row.does}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        ))}
      </div>
    </Sheet>
  );
}
