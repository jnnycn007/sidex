/*---------------------------------------------------------------------------------------------
 *  SideX — Remote Explorer pane.
 *
 *  Shows SSH hosts, WSL distros, Dev Containers, GitHub Codespaces, and
 *  Tunnels in a tree that mirrors VS Code's Remote Explorer.  Each section
 *  can be expanded / collapsed; items have contextual action buttons.
 *--------------------------------------------------------------------------------------------*/

import './media/remoteExplorer.css';

import { localize } from '../../../../nls.js';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { IViewletViewOptions, ViewPane } from '../../../browser/parts/views/viewPane.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';
import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IContextKeyService } from '../../../../platform/contextkey/common/contextkey.js';
import { IViewDescriptorService } from '../../../common/views.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IOpenerService } from '../../../../platform/opener/common/opener.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { IHoverService } from '../../../../platform/hover/browser/hover.js';
import { append, $, addDisposableListener, EventType, clearNode } from '../../../../base/browser/dom.js';
import { ISideXRemoteService, SshHost, WslDistro, ContainerEntry, CodespaceEntry, RemoteKind } from '../../../../platform/sidex/browser/sidexRemoteService.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { Codicon } from '../../../../base/common/codicons.js';
import { ThemeIcon } from '../../../../base/common/themables.js';
import { IQuickInputService } from '../../../../platform/quickinput/common/quickInput.js';
import { localize2 } from '../../../../nls.js';

export class RemoteExplorerViewPane extends ViewPane {
	static readonly ID = 'workbench.view.remoteExplorer';
	static readonly NAME = localize2('remoteExplorer', 'Remote Explorer');

	private container: HTMLElement | undefined;
	private listContainer: HTMLElement | undefined;

	constructor(
		options: IViewletViewOptions,
		@IKeybindingService keybindingService: IKeybindingService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IConfigurationService configurationService: IConfigurationService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IViewDescriptorService viewDescriptorService: IViewDescriptorService,
		@IInstantiationService instantiationService: IInstantiationService,
		@IOpenerService openerService: IOpenerService,
		@IThemeService themeService: IThemeService,
		@IHoverService hoverService: IHoverService,
		@ISideXRemoteService private readonly remoteService: ISideXRemoteService,
		@ICommandService private readonly commandService: ICommandService,
		@IQuickInputService private readonly quickInputService: IQuickInputService,
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, hoverService);
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);
		container.classList.add('remote-explorer-view');
		this.container = container;
		this.listContainer = append(container, $('.remote-explorer-list'));
		this.refresh();
	}

	protected override layoutBody(height: number, width: number): void {
		super.layoutBody(height, width);
	}

	async refresh(): Promise<void> {
		if (!this.listContainer) { return; }
		clearNode(this.listContainer);
		append(this.listContainer, $('div.remote-loading', undefined, localize('remote.loading', 'Loading…')));

		// Pull the stored GitHub token through an in-memory cache so we
		// only hit the OS keychain once per session. Each keychain access
		// triggers a macOS authorization prompt for unsigned dev builds,
		// so we want to minimize them.
		const githubToken = await getCachedGitHubToken();

		const [sshHosts, wslDistros, containers, activeConns, codespaces] = await Promise.allSettled([
			this.remoteService.listSshHosts(),
			this.remoteService.listWslDistros(),
			this.remoteService.listContainers(),
			this.remoteService.activeConnections(),
			githubToken ? this.remoteService.listCodespaces(githubToken) : Promise.resolve([]),
		]);

		clearNode(this.listContainer);
		const active = activeConns.status === 'fulfilled' ? activeConns.value : [];

		// --- Tunnels section ---
		const tunnelSection = this.renderSection(this.listContainer, localize('remote.tunnels', 'Tunnels'), Codicon.remote);
		const activeTunnels = active.filter(c => c.kind === 'tunnel');
		if (activeTunnels.length > 0) {
			for (const t of activeTunnels) {
				this.renderRemoteRow(
					tunnelSection,
					t.label,
					Codicon.remote,
					true,
					() => { /* already connected — no-op */ },
				);
			}
		}
		// Always show sign-in buttons so users can add more auth providers
		this.renderSignInButton(
			tunnelSection,
			localize('remote.signInMicrosoft', 'Sign in to tunnels registered with Microsoft'),
			'microsoft',
			() => this.commandService.executeCommand('sidex.remote.signInTunnel', 'microsoft'),
		);
		if (!githubToken) {
			this.renderSignInButton(
				tunnelSection,
				localize('remote.signInGitHub', 'Sign in to tunnels registered with GitHub'),
				'github',
				() => this.commandService.executeCommand('sidex.remote.signInTunnel', 'github'),
			);
		}

		// --- SSH section ---
		const hosts = sshHosts.status === 'fulfilled' ? sshHosts.value : [];
		const sshSection = this.renderSection(this.listContainer, localize('remote.ssh', 'SSH'), Codicon.remoteExplorer);
		if (hosts.length === 0) {
			this.renderEmptyMessage(sshSection, localize('remote.ssh.noHosts', 'No SSH targets found in ~/.ssh/config'));
		} else {
			for (const host of hosts) {
				this.renderSshHost(sshSection, host, active);
			}
		}

		// --- Codespaces section ---
		const spaces = codespaces.status === 'fulfilled' ? codespaces.value : [];
		const codespaceSection = this.renderSection(this.listContainer, localize('remote.codespaces', 'GitHub Codespaces'), Codicon.github);
		if (!githubToken) {
			this.renderSignInButton(
				codespaceSection,
				localize('remote.codespaces.signIn', 'Sign in with GitHub to see your Codespaces'),
				'github',
				() => this.commandService.executeCommand('sidex.remote.signInTunnel', 'github'),
			);
		} else if (spaces.length === 0) {
			this.renderEmptyMessage(codespaceSection, localize('remote.codespaces.empty', 'No Codespaces found'));
		} else {
			for (const space of spaces) {
				this.renderCodespace(codespaceSection, space, githubToken);
			}
		}

		// --- WSL section (Windows only — other platforms return []) ---
		const distros = wslDistros.status === 'fulfilled' ? wslDistros.value : [];
		if (distros.length > 0) {
			const wslSection = this.renderSection(this.listContainer, localize('remote.wsl', 'WSL Targets'), Codicon.vm);
			for (const distro of distros) {
				this.renderWslDistro(wslSection, distro);
			}
		}

		// --- Dev Containers section ---
		const ctrs = containers.status === 'fulfilled' ? containers.value : [];
		const containerSection = this.renderSection(this.listContainer, localize('remote.containers', 'Dev Containers'), Codicon.package);
		if (ctrs.length === 0) {
			this.renderEmptyMessage(containerSection, localize('remote.containers.noContainers', 'No running containers found'));
		} else {
			for (const ctr of ctrs) {
				this.renderContainer(containerSection, ctr);
			}
		}
	}

	// ── Section helpers ────────────────────────────────────────────────────────

	private renderSection(parent: HTMLElement, label: string, icon: ThemeIcon): HTMLElement {
		const section = append(parent, $('.remote-section'));
		const header = append(section, $('.remote-section-header'));
		header.classList.add('expanded');

		const toggle = append(header, $(ThemeIcon.asCSSSelector(Codicon.chevronDown)));
		toggle.classList.add('remote-section-toggle');
		append(header, $(ThemeIcon.asCSSSelector(icon)));
		append(header, $('span.remote-section-label', undefined, label));

		const body = append(section, $('.remote-section-body'));

		this._register(addDisposableListener(header, EventType.CLICK, () => {
			const expanded = header.classList.toggle('expanded');
			body.style.display = expanded ? '' : 'none';
			toggle.className = expanded
				? ThemeIcon.asCSSSelector(Codicon.chevronDown).slice(1)
				: ThemeIcon.asCSSSelector(Codicon.chevronRight).slice(1);
			toggle.classList.add('remote-section-toggle');
		}));

		return body;
	}

	private renderSignInButton(parent: HTMLElement, label: string, provider: string, action: () => void): void {
		const row = append(parent, $('.remote-sign-in-row'));
		const providerIcon = provider === 'github'
			? Codicon.github
			: Codicon.account;
		append(row, $(ThemeIcon.asCSSSelector(providerIcon)));
		const btn = append(row, $('span.remote-sign-in-label', undefined, label));
		btn.setAttribute('role', 'button');
		btn.tabIndex = 0;
		this._register(addDisposableListener(row, EventType.CLICK, action));
		this._register(addDisposableListener(btn, 'keydown', (e: KeyboardEvent) => {
			if (e.key === 'Enter' || e.key === ' ') { action(); }
		}));
	}

	private renderEmptyMessage(parent: HTMLElement, message: string): void {
		append(parent, $('div.remote-empty-message', undefined, message));
	}

	private renderSshHost(parent: HTMLElement, host: SshHost, activeConns: { label: string; kind: string }[]): void {
		const isConnected = activeConns.some(c => c.kind === 'ssh' && c.label.includes(host.host));
		const row = this.renderRemoteRow(
			parent,
			`${host.user ? `${host.user}@` : ''}${host.host}${host.port ? `:${host.port}` : ''}`,
			Codicon.vm,
			isConnected,
			() => this.commandService.executeCommand('sidex.remote.connect', 'ssh' as RemoteKind, host),
		);
		if (isConnected) {
			row.classList.add('connected');
		}
	}

	private renderWslDistro(parent: HTMLElement, distro: WslDistro): void {
		this.renderRemoteRow(
			parent,
			`${distro.name}${distro.isDefault ? ' (Default)' : ''}`,
			Codicon.terminalLinux,
			false,
			() => this.commandService.executeCommand('sidex.remote.connect', 'wsl' as RemoteKind, distro),
		);
	}

	private renderContainer(parent: HTMLElement, ctr: ContainerEntry): void {
		const isRunning = ctr.status.toLowerCase().includes('up') || ctr.status.toLowerCase().includes('running');
		this.renderRemoteRow(
			parent,
			ctr.name.replace(/^\//, ''),
			Codicon.package,
			isRunning,
			() => isRunning
				? this.commandService.executeCommand('sidex.remote.connect', 'container' as RemoteKind, ctr)
				: undefined,
		);
	}

	private renderCodespace(parent: HTMLElement, space: CodespaceEntry, token: string): void {
		const isRunning = space.state.toLowerCase().includes('available') || space.state.toLowerCase().includes('running');
		this.renderRemoteRow(
			parent,
			space.displayName,
			Codicon.github,
			isRunning,
			async () => {
				try {
					await this.remoteService.connectCodespace(space.name, token);
				} catch (err) {
					this.commandService.executeCommand('sidex.notify.error', String(err));
				}
			},
		);
	}

	private renderRemoteRow(
		parent: HTMLElement,
		label: string,
		icon: ThemeIcon,
		active: boolean,
		onConnect: () => void,
	): HTMLElement {
		const row = append(parent, $('.remote-row'));
		if (active) { row.classList.add('active'); }

		append(row, $(ThemeIcon.asCSSSelector(icon)));
		const labelEl = append(row, $('span.remote-row-label', undefined, label));
		labelEl.title = label;

		const actions = append(row, $('.remote-row-actions'));
		const connectBtn = append(actions, $('a.remote-action-connect'));
		connectBtn.title = localize('remote.connect', 'Connect');
		append(connectBtn, $(ThemeIcon.asCSSSelector(active ? Codicon.check : Codicon.plug)));

		this._register(addDisposableListener(row, EventType.DBLCLICK, onConnect));
		this._register(addDisposableListener(connectBtn, EventType.CLICK, (e) => {
			e.stopPropagation();
			onConnect();
		}));

		return row;
	}
}

// ── Session-scoped cache for the stored GitHub token ─────────────────────────

/**
 * Caches the GitHub device-flow token for the lifetime of the window.
 * Each keychain access triggers a macOS "allow keychain access" prompt
 * on unsigned dev builds, so we read it once and reuse the value.  The
 * module-level cache is fine since the token is user-scoped and
 * cleared only on window reload (same lifecycle as the gallery service).
 */
let _githubTokenCache: { value: string | null; fetched: boolean } = { value: null, fetched: false };

export async function getCachedGitHubToken(): Promise<string | null> {
	if (_githubTokenCache.fetched) {
		return _githubTokenCache.value;
	}
	try {
		const { invoke } = await import('@tauri-apps/api/core');
		const token = (await invoke<string | null>('secret_get', {
			key: 'sidex.remote.github.device-flow',
		})) ?? null;
		_githubTokenCache = { value: token, fetched: true };
		return token;
	} catch {
		_githubTokenCache = { value: null, fetched: true };
		return null;
	}
}

/** Invalidate the cache — call after sign-in so the next read picks up the new token. */
export function clearCachedGitHubToken(): void {
	_githubTokenCache = { value: null, fetched: false };
}