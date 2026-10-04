// OpenRouter as a clean-up service: the recommended models and what a
// dictation costs on each. The `~…-latest` ids follow each family's newest
// version, so the list doesn't go stale when a provider ships a new model.
import type { ModelInfo } from "./api";

export const OPENROUTER = {
  base_url: "https://openrouter.ai/api/v1",
  model: "~deepseek/deepseek-flash-latest",
};

/** Recommended models; `note` is a key under moqi.settings.openrouterNotes. */
export const OPENROUTER_PRESETS: { id: string; name: string; note: string }[] =
  [
    {
      id: "~deepseek/deepseek-flash-latest",
      name: "DeepSeek Flash",
      note: "deepseek",
    },
    { id: "~google/gemini-flash-latest", name: "Gemini Flash", note: "gemini" },
    { id: "~openai/gpt-luna-latest", name: "GPT Luna", note: "luna" },
    {
      id: "~anthropic/claude-haiku-latest",
      name: "Claude Haiku",
      note: "haiku",
    },
    { id: "qwen/qwen3.8-flash", name: "Qwen Flash", note: "qwen" },
  ];

/** A dictation sends the clean-up instructions plus the text (about 1,500
 * tokens) and gets about 100 back. */
const TOKENS_IN = 1500;
const TOKENS_OUT = 100;

/** US$ per 1,000 dictations, from prices in US$ per million tokens. */
export const costPerThousand = (
  m: Pick<ModelInfo, "input_price" | "output_price">,
) => (TOKENS_IN * m.input_price + TOKENS_OUT * m.output_price) / 1000;

export const formatCost = (usd: number) =>
  usd >= 10 ? usd.toFixed(0) : usd >= 1 ? usd.toFixed(1) : usd.toFixed(2);
