import { useCallback, useEffect, useRef, useState } from "react";
import {
  type AssistantProblem,
  type AssistantStatus,
  asProblem,
  assistantConfigure,
  assistantModels,
  assistantSetKey,
  assistantTest,
  type EngineStatus,
  openLink,
} from "../lib/engine";
import { Button } from "./Button";
import { Lcd } from "./Lcd";
import { Select } from "./Select";
import { TextInput } from "./TextInput";

/** Where the common providers' OpenAI-compatible APIs are. */
export const PROVIDERS: { name: string; endpoint: string; local: boolean }[] = [
  { name: "OpenAI", endpoint: "https://api.openai.com/v1", local: false },
  { name: "Anthropic", endpoint: "https://api.anthropic.com/v1", local: false },
  { name: "OpenRouter", endpoint: "https://openrouter.ai/api/v1", local: false },
  { name: "Ollama (local)", endpoint: "http://localhost:11434/v1", local: true },
  { name: "LM Studio (local)", endpoint: "http://localhost:1234/v1", local: true },
];

type Props = {
  assistant: AssistantStatus;
  onStatusChange: (status: EngineStatus) => void;
  /** Scrolls the section into view as it appears. */
  reveal?: boolean;
};

type Check = { state: "idle" } | { state: "testing" } | { state: "ok" } | AssistantProblemCheck;
type AssistantProblemCheck = { state: "problem"; problem: AssistantProblem };

/**
 * The assistant's section of Settings: an enable switch, then the
 * endpoint, the API key, the model and a connection test. The key goes
 * to the shell, which keeps it in the Keychain; it is never shown again.
 */
export function AssistantSettings({ assistant, onStatusChange, reveal = false }: Props) {
  const section = useRef<HTMLElement>(null);
  const revealOnOpen = useRef(reveal);
  useEffect(() => {
    if (revealOnOpen.current) section.current?.scrollIntoView({ block: "start" });
  }, []);
  const [endpoint, setEndpoint] = useState(assistant.endpoint);
  const [key, setKey] = useState("");
  const [models, setModels] = useState<string[] | null>(null);
  const [listProblem, setListProblem] = useState<AssistantProblem | null>(null);
  const [check, setCheck] = useState<Check>({ state: "idle" });

  useEffect(() => setEndpoint(assistant.endpoint), [assistant.endpoint]);

  const configure = useCallback(
    (change: Partial<AssistantStatus>) => {
      setCheck({ state: "idle" });
      const next = { ...assistant, ...change };
      return assistantConfigure({
        enabled: next.enabled,
        endpoint: next.endpoint,
        model: next.model,
        persona: next.persona,
        sounds: next.sounds,
      }).then(onStatusChange);
    },
    [assistant, onStatusChange],
  );

  // The model list follows the endpoint and the key; nothing is asked while off.
  const { enabled, hasKey } = assistant;
  const savedEndpoint = assistant.endpoint;
  useEffect(() => {
    if (!enabled || !savedEndpoint) {
      setModels(null);
      setListProblem(null);
      return;
    }
    let live = true;
    setModels(null);
    setListProblem(null);
    void hasKey;
    assistantModels().then(
      (list) => live && setModels(list),
      (e) => live && setListProblem(asProblem(e)),
    );
    return () => {
      live = false;
    };
  }, [enabled, savedEndpoint, hasKey]);

  const saveEndpoint = () => {
    if (endpoint.trim() !== assistant.endpoint) configure({ endpoint: endpoint.trim() });
  };
  const saveKey = () => {
    assistantSetKey(key).then((s) => {
      setKey("");
      setCheck({ state: "idle" });
      onStatusChange(s);
    });
  };
  const test = () => {
    setCheck({ state: "testing" });
    assistantTest().then(
      () => setCheck({ state: "ok" }),
      (e) => setCheck({ state: "problem", problem: asProblem(e) }),
    );
  };
  const preset = PROVIDERS.find((p) => p.endpoint === assistant.endpoint);

  return (
    <section ref={section} aria-labelledby="assistant-label" className="grid gap-2">
      <h3 id="assistant-label" className="caption m-0">
        Assistant
        <small className="hint block text-ink-dim">
          A chat that can explain, build and fix things in your project, with a model you connect.
          Nothing is sent anywhere until you turn it on.{" "}
          <button
            type="button"
            onClick={() => void openLink("assistant")}
            className="cursor-pointer border-0 bg-transparent p-0 text-label underline"
          >
            What is sent
          </button>
        </small>
      </h3>
      <div className="flex">
        <Button
          aria-pressed={assistant.enabled}
          onClick={() => configure({ enabled: !assistant.enabled })}
        >
          {assistant.enabled ? "Assistant on" : "Assistant off"}
        </Button>
      </div>
      {assistant.enabled ? (
        <>
          <Select
            id="assistant-provider"
            label="Provider"
            hint="Any OpenAI-compatible API works."
            value={preset?.endpoint ?? ""}
            onChange={(e) => e.target.value && configure({ endpoint: e.target.value, model: "" })}
          >
            <option value="">Other</option>
            {PROVIDERS.map((p) => (
              <option key={p.endpoint} value={p.endpoint}>
                {p.name}
              </option>
            ))}
          </Select>
          <TextInput
            id="assistant-endpoint"
            label="Endpoint URL"
            hint="The API's base, usually ending in /v1."
            placeholder="https://api.openai.com/v1"
            spellCheck={false}
            autoComplete="off"
            value={endpoint}
            onChange={(e) => setEndpoint(e.target.value)}
            onBlur={saveEndpoint}
            onKeyDown={(e) => e.key === "Enter" && saveEndpoint()}
          />
          <form
            className="grid gap-1"
            onSubmit={(e) => {
              e.preventDefault();
              if (key.trim()) saveKey();
            }}
          >
            <TextInput
              id="assistant-key"
              type="password"
              label="API key"
              hint={
                assistant.hasKey
                  ? "Saved in your Keychain. Type a new one to replace it."
                  : preset?.local
                    ? "A local model needs none."
                    : "Kept in your Keychain, never in a file."
              }
              placeholder={assistant.hasKey ? "••••••••" : "sk-…"}
              autoComplete="off"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <div className="flex justify-end gap-1">
              {assistant.hasKey ? (
                <Button onClick={() => assistantSetKey("").then(onStatusChange)}>Remove key</Button>
              ) : null}
              <Button type="submit" disabled={!key.trim()}>
                Save key
              </Button>
            </div>
          </form>
          {models && models.length > 0 ? (
            <Select
              id="assistant-model"
              label="Model"
              value={assistant.model}
              onChange={(e) => configure({ model: e.target.value })}
            >
              {assistant.model && !models.includes(assistant.model) ? (
                <option value={assistant.model}>{assistant.model}</option>
              ) : null}
              {assistant.model ? null : <option value="">Pick a model</option>}
              {models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </Select>
          ) : (
            <ModelField
              model={assistant.model}
              loading={models === null && listProblem === null && !!assistant.endpoint}
              onChange={(model) => configure({ model })}
            />
          )}
          <div className="flex flex-wrap items-center gap-1">
            <Button
              aria-pressed={assistant.persona}
              onClick={() => configure({ persona: !assistant.persona })}
              title="Talks as Tapster, the house droid. Off gives plain replies."
            >
              Droid persona
            </Button>
            <Button
              aria-pressed={assistant.sounds}
              onClick={() => configure({ sounds: !assistant.sounds })}
              title="Beeps and chirps on a reply and on an error, in the window only."
            >
              Droid sounds
            </Button>
          </div>
          <div className="flex items-center gap-2">
            <Lcd
              aria-live="polite"
              error={check.state === "problem" || (check.state === "idle" && listProblem !== null)}
              className="min-w-0 flex-1 text-readout"
            >
              {checkText(check, listProblem)}
            </Lcd>
            <Button disabled={check.state === "testing"} onClick={test} className="shrink-0">
              {check.state === "testing" ? "Testing…" : "Test connection"}
            </Button>
          </div>
        </>
      ) : null}
    </section>
  );
}

/** A typed model id, for an endpoint whose list could not be read. */
function ModelField({
  model,
  loading,
  onChange,
}: {
  model: string;
  loading: boolean;
  onChange: (model: string) => void;
}) {
  const [value, setValue] = useState(model);
  useEffect(() => setValue(model), [model]);
  const save = () => value.trim() !== model && onChange(value.trim());
  return (
    <TextInput
      id="assistant-model"
      label="Model"
      hint={loading ? "Asking the endpoint for its models…" : "Its id at the endpoint."}
      spellCheck={false}
      autoComplete="off"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onBlur={save}
      onKeyDown={(e) => e.key === "Enter" && save()}
    />
  );
}

/** The readout beside Test connection. */
function checkText(check: Check, listProblem: AssistantProblem | null): string {
  switch (check.state) {
    case "testing":
      return "Testing…";
    case "ok":
      return "Connected. The assistant is ready.";
    case "problem":
      return check.problem.message;
    default:
      return listProblem?.message ?? "Not tested yet";
  }
}
