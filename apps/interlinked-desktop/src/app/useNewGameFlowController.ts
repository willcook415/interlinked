import { useCallback, useEffect, useState } from "react";
import type { CityOption, CountryOption, CountryPackStatus, CurrencyCode, Difficulty } from "../types";

type NextStepContext = {
  selectedCountry: CountryOption | null;
  selectedCity: CityOption | null;
  selectedCountryPack: CountryPackStatus | null;
  setError: (value: string | null) => void;
};

export function useNewGameFlowController(args: {
  defaultBudgetFor: (difficulty: Difficulty, currency: CurrencyCode) => number;
}) {
  const [step, setStep] = useState<1 | 2 | 3 | 4>(1);
  const [name, setName] = useState("Interlinked World");
  const [intent, setIntent] = useState("balanced");
  const [difficulty, setDifficulty] = useState<Difficulty>("standard");
  const [currency, setCurrency] = useState<CurrencyCode>("GBP");
  const [budget, setBudget] = useState("1500000000");
  const [budgetEdited, setBudgetEdited] = useState(false);

  useEffect(() => {
    if (budgetEdited) return;
    setBudget(String(args.defaultBudgetFor(difficulty, currency)));
  }, [args.defaultBudgetFor, budgetEdited, currency, difficulty]);

  const beginFlow = useCallback(() => {
    setStep(1);
    setIntent("balanced");
    setBudgetEdited(false);
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
        setStep(2);
        return;
      }
      if (step === 2) {
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
        setStep(3);
        return;
      }
      if (step === 3) {
        const numericBudget = Number(budget);
        if (!Number.isFinite(numericBudget) || numericBudget <= 0) {
          context.setError("Enter a valid starting budget.");
          return;
        }
        setStep(4);
      }
    },
    [budget, name, step]
  );

  const previousStep = useCallback(() => {
    setStep((current) => {
      if (current === 4) return 3;
      if (current === 3) return 2;
      return 1;
    });
  }, []);

  const onDifficultyChanged = useCallback((value: Difficulty) => {
    setBudgetEdited(false);
    setDifficulty(value);
  }, []);

  const onCurrencyChanged = useCallback((value: CurrencyCode) => {
    setBudgetEdited(false);
    setCurrency(value);
  }, []);

  const onBudgetChanged = useCallback((value: string) => {
    setBudgetEdited(true);
    setBudget(value);
  }, []);

  return {
    step,
    setStep,
    name,
    setName,
    intent,
    setIntent,
    difficulty,
    currency,
    budget,
    beginFlow,
    nextStep,
    previousStep,
    onDifficultyChanged,
    onCurrencyChanged,
    onBudgetChanged,
  };
}

export type NewGameFlowController = ReturnType<typeof useNewGameFlowController>;
