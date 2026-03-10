import type { JsonValue } from "$lib/backend/generated/serde_json/JsonValue";

export interface FieldConditionAuthState {
  isAuthenticated: boolean;
  tier: string;
}

function parseConditionLiteral(raw: string): JsonValue {
  const value = raw.trim();
  if (value === "true") return true;
  if (value === "false") return false;
  if (value === "null") return null;

  const asNumber = Number(value);
  if (value !== "" && Number.isFinite(asNumber) && String(asNumber) === value) {
    return asNumber;
  }

  return value;
}

function matchesConfigCondition(
  condition: string,
  config: Record<string, JsonValue>,
): boolean | null {
  if (!condition.startsWith("config:")) return null;

  const expression = condition.slice("config:".length).trim();
  if (!expression) return false;

  const operator = expression.includes("!=") ? "!=" : expression.includes("=") ? "=" : null;
  if (!operator) {
    return Boolean(config[expression]);
  }

  const [rawKey, ...rest] = expression.split(operator);
  const key = rawKey?.trim();
  const expected = rest.join(operator).trim();
  if (!key || !expected) return false;

  const actual = config[key];
  const matches = actual === parseConditionLiteral(expected);
  return operator === "=" ? matches : !matches;
}

export function evaluateFieldCondition(
  condition: string,
  authState: FieldConditionAuthState,
  config: Record<string, JsonValue>,
): boolean {
  switch (condition) {
    case "authenticated":
      return authState.isAuthenticated;
    case "not_authenticated":
      return !authState.isAuthenticated;
    case "plus":
      return authState.tier === "plus";
    case "not_plus":
      return authState.tier !== "plus";
    default: {
      const configMatch = matchesConfigCondition(condition, config);
      return configMatch ?? false;
    }
  }
}
