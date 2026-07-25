import type { WriteSignal as MaverickWriteSignal } from "@maverick-js/signals";

import type { SignalId } from "../signal";
import type { Bool } from "./bool";
import { F64 } from "./f64";
import { Ref } from "./ref";
// biome-ignore lint/suspicious/noShadowRestrictedNames: Surrogate type
import { type Str, String } from "./string";

export class WriteSignal<T> {
	constructor(
		private readonly id: SignalId,
		private readonly inner: MaverickWriteSignal<T>,
	) {}

	read(): Ref<T> {
		return new Ref(
			() => this.inner(),
			(v) => this.inner.set(v),
		);
	}

	get(): T {
		const value = this.read().deref() as { clone?: () => T };
		return typeof value?.clone === "function" ? value.clone() : (value as T);
	}

	set(v: T): void {
		this.inner.set(v);
	}

	/** Flips a `Signal<bool>`'s value. */
	toggle(): void {
		this.inner.set((this.inner() as unknown as Bool).not() as unknown as T);
	}

	/** Adds `1` to a `Signal<f64>`'s value. */
	increment(): void {
		this.inner.set((this.inner() as unknown as F64).add(new F64(1)) as unknown as T);
	}

	/** Subtracts `1` from a `Signal<f64>`'s value. */
	decrement(): void {
		this.inner.set((this.inner() as unknown as F64).sub(new F64(1)) as unknown as T);
	}

	/** Appends a string to a `Signal<String>`'s value. */
	push_str(s: Str): void {
		const current = this.inner() as unknown as Str;
		this.inner.set(new String(current.dehydrate() + s.dehydrate()) as unknown as T);
	}

	dehydrate(): { t: "Signal"; id: SignalId } {
		return { t: "Signal", id: this.id };
	}
}
