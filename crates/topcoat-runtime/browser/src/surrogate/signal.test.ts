import { expect, it } from "vitest";

import { SignalRegistry } from "../signal";
import { Bool } from "./bool";
import { F64 } from "./f64";
import { WriteSignal } from "./signal";
// biome-ignore lint/suspicious/noShadowRestrictedNames: Surrogate type
import { Str, String } from "./string";

function signalOf<T>(value: T): WriteSignal<T> {
	const registry = new SignalRegistry();
	registry.insert("test", value);
	return new WriteSignal<T>("test", registry.handle("test") as never);
}

it("toggle flips a bool signal", () => {
	const signal = signalOf(new Bool(false));

	signal.toggle();
	expect(signal.get().dehydrate()).toBe(true);

	signal.toggle();
	expect(signal.get().dehydrate()).toBe(false);
});

it("increment and decrement step an f64 signal by one", () => {
	const signal = signalOf(new F64(41));

	signal.increment();
	expect(signal.get().dehydrate()).toBe(42);

	signal.decrement();
	signal.decrement();
	expect(signal.get().dehydrate()).toBe(40);
});

it("push_str appends to a string signal", () => {
	const signal = signalOf(new String("Hello"));

	signal.push_str(new Str(", world"));
	signal.push_str(new String("!"));

	expect(signal.get().dehydrate()).toBe("Hello, world!");
});
