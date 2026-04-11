import { useCallback, useEffect, useState } from "react";
import type { CityOption, CountryOption, CountryPackStatus, CurrencyCode, Difficulty } from "../types";

type NextStepContext = {
  selectedCountry: CountryOption | null;
  selectedCity: CityOption | null;
  selectedCountryPack: CountryPackStatus | null;
  setError: (value: string | null) => void;
};

function parsePositiveBudget(value: string): number | null {
  const normalized = value.replace(/[^\d.-]/g, "").trim();
  if (!normalized) return null;
  const parsed = Number(normalized);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
}

export function useNewGameFlowController(args: {
  defaultBudgetFor: (difficulty: Difficulty, currency: CurrencyCode) => number;
}) {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [name, setName] = useState("Interlinked World");
  const [difficulty, setDifficulty] = useState<Difficulty>("standard");
  const [currency, setCurrency] = useState<CurrencyCode>("GBP");
  const [budget, setBudget] = useState("1500000000");

  useEffect(() => {
    setBudget(String(args.defaultBudgetFor(difficulty, currency)));
  }, [args.defaultBudgetFor, currency, difficulty]);

  const beginFlow = useCallback(() => {
    setStep(1);
    setBudget(String(args.defaultBudgetFor(difficulty, currency)));
  }, [args.defaultBudgetFor, currency, difficulty]);

  const nextStep = useCallback(
    (context: NextStepContext) => {
      context.setError(null);
      if (step === 1) {
        if (!name.trim()) {
          context.setError("Enter a game name.");
          return;
        }
        if (!context.selectedCountry || !context.selectedCity) {
          context.setError("Select country and city.");
          return;
        }
        if (!context.selectedCountryPack?.eligible) {
          context.setError(
            context.selectedCountryPack?.reason ??
              `Country ${context.selectedCountry.iso2} is not available yet.`
          );
          return;
        }
        setStep(2);
        return;
      }
      if (step === 2) {
        const numericBudget = parsePositiveBudget(budget);
        if (numericBudget === null) {
          context.setError("Enter a valid starting budget.");
          return;
        }
        setStep(3);
        return;
      }
    },
    [budget, name, step]
  );

  const previousStep = useCallback(() => {
    setStep((current) => {
      if (current === 3) return 2;
      if (current === 2) return 1;
      return 1;
    });
  }, []);

  const onDifficultyChanged = useCallback((value: Difficulty) => {
    setDifficulty(value);
  }, []);

  const onCurrencyChanged = useCallback((value: CurrencyCode) => {
    setCurrency(value);
  }, []);

  return {
    step,
    setStep,
    name,
    setName,
    difficulty,
    currency,
    budget,
    beginFlow,
    nextStep,
    previousStep,
    onDifficultyChanged,
    onCurrencyChanged,
  };
}

export type NewGameFlowController = ReturnType<typeof useNewGameFlowController>;
