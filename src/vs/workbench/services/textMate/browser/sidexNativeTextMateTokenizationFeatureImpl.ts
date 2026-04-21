/*---------------------------------------------------------------------------------------------
 *  SideX — Native Rust textmate tokenization, wired as ITextMateTokenizationService.
 *--------------------------------------------------------------------------------------------*/

import * as domStylesheets from '../../../../base/browser/domStylesheets.js';
import { equals as equalArray } from '../../../../base/common/arrays.js';
import { Color } from '../../../../base/common/color.js';
import { Disposable, DisposableStore, IDisposable } from '../../../../base/common/lifecycle.js';
import { IObservable, observableFromEvent } from '../../../../base/common/observable.js';
import { URI } from '../../../../base/common/uri.js';
import { LanguageId } from '../../../../editor/common/encodedTokenAttributes.js';
import {
	EncodedTokenizationResult,
	IBackgroundTokenizationStore,
	IBackgroundTokenizer,
	IState,
	ITokenizationSupport,
	LazyTokenizationSupport,
	TokenizationRegistry,
	TokenizationResult,
} from '../../../../editor/common/languages.js';
import { ILanguageService } from '../../../../editor/common/languages/language.js';
import { nullTokenizeEncoded } from '../../../../editor/common/languages/nullTokenize.js';
import {
	generateTokensCSSForColorMap,
	generateTokensCSSForFontMap,
} from '../../../../editor/common/languages/supports/tokenization.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IExtensionResourceLoaderService } from '../../../../platform/extensionResourceLoader/common/extensionResourceLoader.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IFontTokenOptions } from '../../../../platform/theme/common/themeService.js';
import { ExtensionMessageCollector, IExtensionPointUser } from '../../extensions/common/extensionsRegistry.js';
import { ITextMateThemingRule, IWorkbenchColorTheme, IWorkbenchThemeService } from '../../themes/common/workbenchThemeService.js';
import { ITMSyntaxExtensionPoint, grammarsExtPoint } from '../common/TMGrammars.js';
import { IValidGrammarDefinition } from '../common/TMScopeRegistry.js';
import * as resources from '../../../../base/common/resources.js';
import * as types from '../../../../base/common/types.js';
import * as nls from '../../../../nls.js';
import { getNativeTextMate } from '../../../../platform/sidex/browser/sidexTextMateService.js';
import { ITextMateTokenizationService } from './textMateTokenizationFeature.js';
import type { IGrammar } from 'vscode-textmate';
import { INotificationService } from '../../../../platform/notification/common/notification.js';

// ---------------------------------------------------------------------------
// Native tokenizer state — wraps an opaque Rust rule-stack handle.
// Handle 0 means "initial state" (no Rust object yet).
// ---------------------------------------------------------------------------

class NativeTokenizerState implements IState {
	static readonly INITIAL = new NativeTokenizerState(0);

	constructor(public readonly handle: number) {}

	clone(): IState {
		return this;
	}

	equals(other: IState): boolean {
		return other instanceof NativeTokenizerState && other.handle === this.handle;
	}
}

// ---------------------------------------------------------------------------
// Per-language tokenization support
// ---------------------------------------------------------------------------

interface PrefetchEntry {
	tokens: Uint32Array;
	nextHandle: number;
}

class SidexNativeTokenizationSupport implements ITokenizationSupport, IDisposable {
	/** Primary cache: `"<handle>:<line>"` → fresh result (consumed once, then cleared). */
	private readonly _cache = new Map<string, PrefetchEntry>();
	/** Stale cache: last-known good tokens for a key, never cleared on read. */
	private readonly _stale = new Map<string, PrefetchEntry>();
	/** Tracks pending async calls so we don't double-fire. */
	private readonly _inflight = new Set<string>();
	/** Pending handleChange — debounced so one burst of misses = one re-render pass. */
	private _changeTimer: ReturnType<typeof setTimeout> | undefined;

	constructor(
		private readonly _languageId: string,
		private readonly _scopeName: string,
		private readonly _encodedLanguageId: LanguageId,
		private readonly _maxTokenizationLineLength: IObservable<number>,
	) {}

	getInitialState(): IState {
		return NativeTokenizerState.INITIAL;
	}

	tokenize(_line: string, _hasEOL: boolean, _state: IState): TokenizationResult {
		throw new Error('SidexNativeTokenizationSupport: plain tokenize() not supported');
	}

	tokenizeEncoded(line: string, _hasEOL: boolean, state: IState): EncodedTokenizationResult {
		const maxLen = this._maxTokenizationLineLength.get();
		if (maxLen > 0 && line.length >= maxLen) {
			return nullTokenizeEncoded(this._encodedLanguageId, state);
		}

		const handle = state instanceof NativeTokenizerState ? state.handle : 0;
		const key = `${handle}:${line}`;

		// Fresh cache hit — consume it and advance the state.
		const cached = this._cache.get(key);
		if (cached) {
			this._cache.delete(key);
			// Update stale cache so future misses have something useful to show.
			this._stale.set(key, cached);
			return new EncodedTokenizationResult(
				cached.tokens,
				[],
				new NativeTokenizerState(cached.nextHandle),
			);
		}

		// Schedule background fetch.
		this._schedulePrefetch(key, handle, line);

		// Stale-while-revalidate: return last-known tokens so the line never
		// goes plain while the async fetch is in flight.  This eliminates the
		// "flicker to uncolored" effect on scroll / file open.
		const stale = this._stale.get(key);
		if (stale) {
			return new EncodedTokenizationResult(
				stale.tokens,
				[],
				new NativeTokenizerState(stale.nextHandle),
			);
		}

		// True first miss — return null tokens for now.
		return nullTokenizeEncoded(this._encodedLanguageId, state);
	}

	createBackgroundTokenizer(
		_textModel: unknown,
		_store: IBackgroundTokenizationStore,
	): IBackgroundTokenizer | undefined {
		return undefined;
	}

	private _schedulePrefetch(key: string, handle: number, line: string): void {
		if (this._inflight.has(key)) {
			return;
		}
		this._inflight.add(key);
		getNativeTextMate()
			.tokenizeLineBinary(this._scopeName, line, handle === 0 ? undefined : handle)
			.then(result => {
				this._inflight.delete(key);
				if (!result) {
					return;
				}
				this._cache.set(key, {
					tokens: new Uint32Array(result.tokens),
					nextHandle: result.ruleStack,
				});
				// Debounce re-tokenize: wait 16 ms (one frame) so a burst of
				// incoming results from fast scrolling fires a single pass
				// instead of one per line.
				if (this._changeTimer !== undefined) {
					clearTimeout(this._changeTimer);
				}
				this._changeTimer = setTimeout(() => {
					this._changeTimer = undefined;
					TokenizationRegistry.handleChange([this._languageId]);
				}, 16);
			})
			.catch(() => {
				this._inflight.delete(key);
			});
	}

	/** Release all Rust stack handles referenced by the cache. */
	dispose(): void {
		if (this._changeTimer !== undefined) {
			clearTimeout(this._changeTimer);
			this._changeTimer = undefined;
		}
		const native = getNativeTextMate();
		for (const entry of this._cache.values()) {
			if (entry.nextHandle !== 0) {
				native.releaseStack(entry.nextHandle);
			}
		}
		this._cache.clear();
		this._stale.clear();
		this._inflight.clear();
	}
}

// ---------------------------------------------------------------------------
// The main service
// ---------------------------------------------------------------------------

export class SidexTextMateTokenizationFeature extends Disposable implements ITextMateTokenizationService {
	public _serviceBrand: undefined;

	private readonly _styleElement: HTMLStyleElement;
	private readonly _tokenizersRegistrations: DisposableStore;

	private _grammarDefinitions: IValidGrammarDefinition[] | null = null;
	private _currentTheme: { name: string; settings: ITextMateThemingRule[] } | null = null;
	private _currentTokenColorMap: string[] | null = null;
	private _currentTokenFontMap: IFontTokenOptions[] | null = null;
	private readonly _createdModes: string[] = [];

	constructor(
		@ILanguageService private readonly _languageService: ILanguageService,
		@IWorkbenchThemeService private readonly _themeService: IWorkbenchThemeService,
		@IExtensionResourceLoaderService private readonly _extensionResourceLoaderService: IExtensionResourceLoaderService,
		@ILogService private readonly _logService: ILogService,
		@IConfigurationService private readonly _configurationService: IConfigurationService,
		@INotificationService private readonly _notificationService: INotificationService,
	) {
		super();

		this._tokenizersRegistrations = this._register(new DisposableStore());
		this._styleElement = domStylesheets.createStyleSheet();
		this._styleElement.className = 'vscode-tokens-styles';

		grammarsExtPoint.setHandler(extensions => {
			this._handleGrammarsExtPoint(extensions);
		});

		this._updateTheme(this._themeService.getColorTheme(), true);
		this._register(
			this._themeService.onDidColorThemeChange(() => {
				this._updateTheme(this._themeService.getColorTheme(), false);
			}),
		);

		this._register(
			this._languageService.onDidRequestRichLanguageFeatures(languageId => {
				this._createdModes.push(languageId);
			}),
		);
	}

	// -------------------------------------------------------------------------
	// ITextMateTokenizationService
	// -------------------------------------------------------------------------

	/** Not used by the native path; returns null. */
	public async createTokenizer(_languageId: string): Promise<IGrammar | null> {
		return null;
	}

	public startDebugMode(_printFn: (str: string) => void, _onStop: () => void): void {
		this._notificationService.info(
			nls.localize('sidex.textmate.noDebug', 'TextMate debug mode is not available with the native Rust tokenizer.'),
		);
	}

	// -------------------------------------------------------------------------
	// Grammar registration
	// -------------------------------------------------------------------------

	private _handleGrammarsExtPoint(
		extensions: readonly IExtensionPointUser<ITMSyntaxExtensionPoint[]>[],
	): void {
		this._grammarDefinitions = null;
		this._tokenizersRegistrations.clear();

		this._grammarDefinitions = [];
		for (const extension of extensions) {
			for (const grammar of extension.value) {
				const validated = this._validateGrammarDefinition(extension, grammar);
				if (!validated) {
					continue;
				}
				this._grammarDefinitions.push(validated);

				if (validated.language) {
					const language = validated.language;
					const scopeName = validated.scopeName;

					const lazySupport = new LazyTokenizationSupport(() =>
						this._createNativeTokenizationSupport(language, scopeName, validated),
					);
					this._tokenizersRegistrations.add(lazySupport);
					this._tokenizersRegistrations.add(
						TokenizationRegistry.registerFactory(language, lazySupport),
					);
				}
			}
		}

		for (const createdMode of this._createdModes) {
			TokenizationRegistry.getOrCreate(createdMode);
		}
	}

	private async _createNativeTokenizationSupport(
		languageId: string,
		scopeName: string,
		def: IValidGrammarDefinition,
	): Promise<(ITokenizationSupport & IDisposable) | null> {
		if (!this._languageService.isRegisteredLanguageId(languageId)) {
			return null;
		}

		try {
			const grammarContent = await this._extensionResourceLoaderService.readExtensionResource(def.location);

			let grammarJson: string;
			if (def.location.path.endsWith('.json')) {
				grammarJson = grammarContent;
			} else {
				// plist / xml grammar — convert via vscode-textmate's parseRawGrammar then re-serialise
				const vscodeTextmate = await import('vscode-textmate');
				const raw = vscodeTextmate.parseRawGrammar(grammarContent, def.location.path);
				grammarJson = JSON.stringify(raw);
			}

			const embeddedLanguages: Record<string, number> = {};
			for (const [scope, id] of Object.entries(def.embeddedLanguages)) {
				embeddedLanguages[scope] = id;
			}

			await getNativeTextMate().loadGrammar({
				scopeName,
				grammarJson,
				initialLanguageId: this._languageService.languageIdCodec.encodeLanguageId(languageId),
				embeddedLanguages: Object.keys(embeddedLanguages).length > 0 ? embeddedLanguages : undefined,
				injectionScopeNames: def.injectTo,
			});
		} catch (err) {
			this._logService.error(`[SideX-TextMate] Failed to load grammar for ${languageId} (${scopeName}):`, err);
			return null;
		}

		const encodedLanguageId = this._languageService.languageIdCodec.encodeLanguageId(languageId);
		const maxTokenizationLineLength = this._observableConfigValue<number>(
			'editor.maxTokenizationLineLength',
			languageId,
			-1,
		);

		return new SidexNativeTokenizationSupport(languageId, scopeName, encodedLanguageId, maxTokenizationLineLength);
	}

	// -------------------------------------------------------------------------
	// Theme handling
	// -------------------------------------------------------------------------

	private _updateTheme(colorTheme: IWorkbenchColorTheme, forceUpdate: boolean): void {
		if (
			!forceUpdate &&
			this._currentTheme &&
			this._currentTokenColorMap &&
			equalsTokenRules(this._currentTheme.settings, colorTheme.tokenColors) &&
			equalArray(this._currentTokenColorMap, colorTheme.tokenColorMap) &&
			this._currentTokenFontMap &&
			equalArray(this._currentTokenFontMap, colorTheme.tokenFontMap)
		) {
			return;
		}

		this._currentTheme = { name: colorTheme.label, settings: colorTheme.tokenColors };
		this._currentTokenColorMap = colorTheme.tokenColorMap;
		this._currentTokenFontMap = colorTheme.tokenFontMap;

		const colorMap = toColorMap(this._currentTokenColorMap);
		const colorCssRules = generateTokensCSSForColorMap(colorMap);
		const fontCssRules = generateTokensCSSForFontMap(this._currentTokenFontMap);
		this._styleElement.textContent = colorCssRules + fontCssRules;
		TokenizationRegistry.setColorMap(colorMap);

		const nativeSettings = colorTheme.tokenColors.map(rule => ({
			name: rule.name,
			scope: rule.scope ?? null,
			settings: rule.settings
				? {
						fontStyle: rule.settings.fontStyle,
						foreground: rule.settings.foreground,
						background: rule.settings.background,
					}
				: {},
		}));

		// Rust expects `colorMap` as `Option<Vec<String>>`; the VS Code
		// token color map can have null/undefined entries at indices
		// with no assigned color. Replace them with an empty string so
		// serde's `Vec<String>` deserializer doesn't reject null values.
		// Rust expects `colorMap` as `Option<Vec<String>>`.  VS Code's
		// tokenColorMap is a *sparse* array — indices with no color are holes
		// (not null), and Array.prototype.map skips holes leaving them as
		// undefined in the output.  Array.from visits every slot, filling
		// holes with undefined, so the callback always fires and we always
		// produce a dense array of strings.
		const rawColorMap = this._currentTokenColorMap ?? [];
		const colorMapArg: string[] = Array.from(
			{ length: rawColorMap.length },
			(_, i) => {
				const c = rawColorMap[i];
				return (c === null || c === undefined) ? '' : String(c);
			},
		);

		// Remove debug block now that we know the cause
		getNativeTextMate()
			.updateTheme(nativeSettings, colorMapArg)
			.catch(err => {
				this._logService.error('[SideX-TextMate] Failed to update theme:', err);
			});
	}

	// -------------------------------------------------------------------------
	// Grammar-definition validation (mirrors the upstream impl)
	// -------------------------------------------------------------------------

	private _validateGrammarDefinition(
		extension: IExtensionPointUser<ITMSyntaxExtensionPoint[]>,
		grammar: ITMSyntaxExtensionPoint,
	): IValidGrammarDefinition | null {
		if (!_validateGrammarExtensionPoint(extension.description.extensionLocation, grammar, extension.collector, this._languageService)) {
			return null;
		}

		const grammarLocation = resources.joinPath(extension.description.extensionLocation, grammar.path);
		const embeddedLanguages: Record<string, LanguageId> = Object.create(null);
		if (grammar.embeddedLanguages) {
			for (const scope of Object.keys(grammar.embeddedLanguages)) {
				const lang = grammar.embeddedLanguages[scope];
				if (typeof lang === 'string' && this._languageService.isRegisteredLanguageId(lang)) {
					embeddedLanguages[scope] = this._languageService.languageIdCodec.encodeLanguageId(lang);
				}
			}
		}

		const tokenTypes: Record<string, number> = Object.create(null);
		if (grammar.tokenTypes) {
			for (const scope of Object.keys(grammar.tokenTypes)) {
				switch (grammar.tokenTypes[scope]) {
					case 'string': tokenTypes[scope] = 2; break;
					case 'other': tokenTypes[scope] = 0; break;
					case 'comment': tokenTypes[scope] = 1; break;
				}
			}
		}

		const validLanguageId =
			grammar.language && this._languageService.isRegisteredLanguageId(grammar.language)
				? grammar.language
				: undefined;

		return {
			location: grammarLocation,
			language: validLanguageId,
			scopeName: grammar.scopeName,
			embeddedLanguages,
			tokenTypes,
			injectTo: grammar.injectTo,
			balancedBracketSelectors: asStringArray(grammar.balancedBracketScopes, ['*']),
			unbalancedBracketSelectors: asStringArray(grammar.unbalancedBracketScopes, []),
			sourceExtensionId: extension.description.id,
		};
	}

	// -------------------------------------------------------------------------
	// Helpers
	// -------------------------------------------------------------------------

	private _observableConfigValue<T>(key: string, languageId: string, defaultValue: T): IObservable<T> {
		return observableFromEvent(
			handleChange =>
				this._configurationService.onDidChangeConfiguration(e => {
					if (e.affectsConfiguration(key, { overrideIdentifier: languageId })) {
						handleChange(e);
					}
				}),
			() => this._configurationService.getValue<T>(key, { overrideIdentifier: languageId }) ?? defaultValue,
		);
	}
}

// ---------------------------------------------------------------------------
// Module-level helpers
// ---------------------------------------------------------------------------

function toColorMap(colorMap: string[]): Color[] {
	const result: Color[] = [null!];
	for (let i = 1, len = colorMap.length; i < len; i++) {
		result[i] = Color.fromHex(colorMap[i]);
	}
	return result;
}

function equalsTokenRules(
	a: ITextMateThemingRule[] | null,
	b: ITextMateThemingRule[] | null,
): boolean {
	if (!b || !a || b.length !== a.length) {
		return false;
	}
	for (let i = b.length - 1; i >= 0; i--) {
		const r1 = b[i];
		const r2 = a[i];
		if (r1.scope !== r2.scope) {
			return false;
		}
		const s1 = r1.settings;
		const s2 = r2.settings;
		if (s1 && s2) {
			if (
				s1.fontStyle !== s2.fontStyle ||
				s1.foreground !== s2.foreground ||
				s1.background !== s2.background
			) {
				return false;
			}
		} else if (!s1 || !s2) {
			return false;
		}
	}
	return true;
}

function asStringArray(value: unknown, defaultValue: string[]): string[] {
	if (!Array.isArray(value) || !value.every(e => typeof e === 'string')) {
		return defaultValue;
	}
	return value as string[];
}

function _validateGrammarExtensionPoint(
	extensionLocation: URI,
	syntax: ITMSyntaxExtensionPoint,
	collector: ExtensionMessageCollector,
	languageService: ILanguageService,
): boolean {
	if (
		syntax.language &&
		(typeof syntax.language !== 'string' || !languageService.isRegisteredLanguageId(syntax.language))
	) {
		collector.error(
			nls.localize('invalid.language', 'Unknown language in `contributes.{0}.language`. Provided value: {1}', 'grammars', String(syntax.language)),
		);
		return false;
	}
	if (!syntax.scopeName || typeof syntax.scopeName !== 'string') {
		collector.error(
			nls.localize('invalid.scopeName', 'Expected string in `contributes.{0}.scopeName`. Provided value: {1}', 'grammars', String(syntax.scopeName)),
		);
		return false;
	}
	if (!syntax.path || typeof syntax.path !== 'string') {
		collector.error(
			nls.localize('invalid.path.0', 'Expected string in `contributes.{0}.path`. Provided value: {1}', 'grammars', String(syntax.path)),
		);
		return false;
	}
	if (
		syntax.injectTo &&
		(!Array.isArray(syntax.injectTo) || syntax.injectTo.some(s => typeof s !== 'string'))
	) {
		collector.error(
			nls.localize('invalid.injectTo', 'Invalid value in `contributes.{0}.injectTo`. Must be an array of language scope names. Provided value: {1}', 'grammars', JSON.stringify(syntax.injectTo)),
		);
		return false;
	}
	if (syntax.embeddedLanguages && !types.isObject(syntax.embeddedLanguages)) {
		collector.error(
			nls.localize('invalid.embeddedLanguages', 'Invalid value in `contributes.{0}.embeddedLanguages`. Must be an object map from scope name to language. Provided value: {1}', 'grammars', JSON.stringify(syntax.embeddedLanguages)),
		);
		return false;
	}
	if (syntax.tokenTypes && !types.isObject(syntax.tokenTypes)) {
		collector.error(
			nls.localize('invalid.tokenTypes', 'Invalid value in `contributes.{0}.tokenTypes`. Must be an object map from scope name to token type. Provided value: {1}', 'grammars', JSON.stringify(syntax.tokenTypes)),
		);
		return false;
	}

	const grammarLocation = resources.joinPath(extensionLocation, syntax.path);
	if (!resources.isEqualOrParent(grammarLocation, extensionLocation)) {
		collector.warn(
			nls.localize(
				'invalid.path.1',
				"Expected `contributes.{0}.path` ({1}) to be included inside extension's folder ({2}). This might make the extension non-portable.",
				'grammars',
				grammarLocation.path,
				extensionLocation.path,
			),
		);
	}
	return true;
}
