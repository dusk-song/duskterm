import { defineStore } from 'pinia';
import { reactive } from 'vue';
import { invokeCommand } from '@/utils/ipc';
import { toast } from '@/composables/useToast';
import { isRedisConnectionError, redisErrorMessage, sameRedisPrimary } from '@/components/redis/redisRecovery';
import { useWorkspaceStore } from './workspace';
import { useSshStore } from './ssh';
import { loadMainUiSettings } from '@/utils/mainUi';

export const isRedisProfile = config => String(config?.protocol || '').trim().toLowerCase() === 'redis';
export const redisModeLabels = { standalone: '单机', sentinel: '哨兵', cluster: '集群' };

export const useRedisStore = defineStore('redis', () => {
  const workspace = useWorkspaceStore();
  const sessions = reactive({});
  const connections = new Map();
  const guards = new Map();
  const closing = new Set();

  async function connect(id, { automatic = false } = {}) {
    const session = sessions[id];
    if (!session || closing.has(id) || connections.has(id)) return;
    const reconnecting = session.generation > 0;
    const key = `redis-connect:${id}`;
    const loadingToast = automatic ? null : toast.loading({
      content: `${session.name} · ${reconnecting ? '正在重连…' : '正在连接…'}`, key, duration: 0,
    });
    session.status = 'connecting';
    session.error = '';
    const pending = (async () => {
      try {
        if (reconnecting) await invokeCommand('redis_disconnect', { id });
        if (session.closing) return;
        const health = await invokeCommand('redis_connect', { id, profileId: session.config.id, endpoint: session.config });
        if (session.closing) return;
        session.primary = health?.primary ?? null;
        session.generation += 1;
        session.status = 'connected';
        if (sessions[id]) {
          toast.success({ content: `${session.name} · ${reconnecting ? '已恢复连接' : '连接成功'}`, key });
        }
        const recent = loadMainUiSettings().recentSessions;
        if (recent.enabled && !automatic) {
          void invokeCommand('record_session_connection', { id: session.config.id, limit: recent.limit })
            .then(() => useSshStore().loadSavedSessions({ force: true }))
            .catch(error => toast.warning(`连接成功，最近会话记录更新失败：${redisErrorMessage(error)}`));
        }
      } catch (error) {
        if (session.closing) return;
        session.error = redisErrorMessage(error);
        session.status = 'disconnected';
        if (sessions[id] && (!automatic || !isRedisConnectionError(error, true))) {
          toast.error({ content: `${session.name} · ${session.error}`, key, duration: 4000 });
        }
      } finally {
        if (session.closing && loadingToast !== null) toast.remove(loadingToast);
      }
    })();
    connections.set(id, pending);
    await pending;
    connections.delete(id);
  }

  function reportFailure(id, generation, cause, includeTimeout = false) {
    const session = sessions[id];
    if (session?.status !== 'connected' || session.generation !== generation ||
      !isRedisConnectionError(cause, includeTimeout)) return;
    session.error = redisErrorMessage(cause);
    session.status = 'disconnected';
    toast.warning({ content: `${session.name} · ${session.error}`, key: `redis-connect:${id}`, duration: 4000 });
  }

  function reportHealth(id, generation, health) {
    const session = sessions[id];
    if (session?.status !== 'connected' || session.generation !== generation ||
      !health?.primary || sameRedisPrimary(session.primary, health.primary)) return;
    const previousPrimary = session.primary;
    session.primary = health.primary;
    session.generation += 1;
    if (previousPrimary) toast.info(`${session.name} · 主节点已切换`);
  }

  function open(config) {
    const id = crypto.randomUUID();
    sessions[id] = { id, kind: 'redis', name: config.name, config, status: 'connecting', primary: null, generation: 0, error: '' };
    workspace.add(id, 'redis');
    void connect(id);
    return id;
  }

  function guardClose(id, guard) {
    if (guard) guards.set(id, guard);
    else guards.delete(id);
  }

  async function close(id) {
    const session = sessions[id];
    if (!session || closing.has(id)) return false;
    closing.add(id);
    try {
      const guard = guards.get(id);
      if (guard && !await guard()) return false;
      session.closing = true;
      await connections.get(id);
      await invokeCommand('redis_disconnect', { id });
      delete sessions[id];
      workspace.remove(id);
      return true;
    } catch (error) {
      session.status = 'disconnected';
      session.error = redisErrorMessage(error);
      toast.error(`关闭 Redis 会话失败：${session.error}`);
      return false;
    } finally {
      session.closing = false;
      closing.delete(id);
    }
  }

  function activate(id) {
    workspace.activeId = id;
  }

  return { sessions, open, connect, reportFailure, reportHealth, close, guardClose, activate };
});
