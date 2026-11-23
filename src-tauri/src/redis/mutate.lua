local key, destination = KEYS[1], KEYS[2]
local action, kind, field, expected, value = ARGV[1], ARGV[2], ARGV[3], ARGV[4], ARGV[5]
local score, expected_score, index, ttl, marker = ARGV[6], tonumber(ARGV[7]), tonumber(ARGV[8]), tonumber(ARGV[9]), ARGV[10]
local expected_size = tonumber(ARGV[11]) or -1
local current = redis.call('TYPE', key).ok
local writes = {}
local function write(...)
    table.insert(writes, {...})
end
local function commit()
    if redis.acl_check_cmd then
        for _, command in ipairs(writes) do
            if not redis.acl_check_cmd(unpack(command)) then
                return redis.error_reply('NOPERM Redis mutation')
            end
        end
    elseif #writes > 1 then
        return redis.error_reply('MUTATEUNSUPPORTED Redis 7')
    end
    for index, command in ipairs(writes) do
        local response = redis.pcall(unpack(command))
        if type(response) == 'table' and response.err then
            if index > 1 then return 'uncertain' end
            return response
        end
    end
    return 'ok'
end
if (action == 'create' and current ~= 'none') or (action ~= 'create' and current ~= kind) then
    return 'conflict'
end
if key ~= destination and redis.call('EXISTS', destination) == 1 then return 'conflict' end
if expected_size >= 0 then
    local size_command = ({string = 'STRLEN', hash = 'HLEN', list = 'LLEN', set = 'SCARD', zset = 'ZCARD'})[kind]
    if redis.call(size_command, key) ~= expected_size then return 'conflict' end
end
if action == 'delete' then
    if kind == 'string' and ARGV[12] == '1' and (redis.call('STRLEN', key) ~= #expected or redis.call('GET', key) ~= expected) then
        return 'conflict'
    end
    write('UNLINK', key)
    return commit()
end
if action == 'create' or action == 'add' then
    if kind == 'string' and action == 'create' then
        write('SET', key, value)
    elseif kind == 'hash' and redis.call('HEXISTS', key, field) == 0 then
        write('HSET', key, field, value)
    elseif kind == 'list' then
        write('RPUSH', key, value)
    elseif kind == 'set' and redis.call('SISMEMBER', key, value) == 0 then
        write('SADD', key, value)
    elseif kind == 'zset' and not redis.call('ZSCORE', key, value) then
        write('ZADD', key, score, value)
    else
        return 'conflict'
    end
elseif action == 'string' then
    if kind ~= 'string' or redis.call('STRLEN', key) ~= #expected or redis.call('GET', key) ~= expected then return 'conflict' end
    write('SET', key, value, 'KEEPTTL')
elseif action == 'update' or action == 'remove' then
    if action == 'remove' and key ~= destination then return 'conflict' end
    if kind == 'hash' then
        if redis.call('HSTRLEN', key, field) ~= #expected or redis.call('HGET', key, field) ~= expected then return 'conflict' end
        if action == 'remove' then write('HDEL', key, field) else write('HSET', key, field, value) end
    elseif kind == 'list' then
        if redis.call('LINDEX', key, index) ~= expected then return 'conflict' end
        if action == 'remove' then
            if redis.call('LLEN', key) > 100000 then return redis.error_reply('WRITELIMIT 列表过长') end
            write('LSET', key, index, marker)
            write('LREM', key, 1, marker)
        else write('LSET', key, index, value) end
    elseif kind == 'set' then
        if redis.call('SISMEMBER', key, expected) == 0 then return 'conflict' end
        if action == 'remove' then write('SREM', key, expected)
        elseif value ~= expected then
            if redis.call('SISMEMBER', key, value) == 1 then return 'conflict' end
            write('SADD', key, value)
            write('SREM', key, expected)
        end
    elseif kind == 'zset' then
        if tonumber(redis.call('ZSCORE', key, expected)) ~= expected_score then return 'conflict' end
        if action == 'remove' then write('ZREM', key, expected)
        else
            if value ~= expected and redis.call('ZSCORE', key, value) then return 'conflict' end
            write('ZADD', key, score, value)
            if value ~= expected then write('ZREM', key, expected) end
        end
    else return 'conflict' end
elseif action ~= 'metadata' then
    return 'conflict'
end
if key ~= destination then write('RENAME', key, destination) end
if ttl == 0 then write('PERSIST', destination)
elseif ttl > 0 then write('PEXPIRE', destination, ttl) end
return commit()
