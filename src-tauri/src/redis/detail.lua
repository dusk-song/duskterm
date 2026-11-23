local key = KEYS[1]
local kind = redis.call('TYPE', key).ok
local start = ARGV[1]
local page = 32
local ttl = redis.call('PTTL', key)
local size = 0
local next_cursor = '0'
local value = ''
local entries = {}
local function row(field, data, score, limit)
    return {string.sub(field, 1, limit), #field, string.sub(data, 1, limit), #data, score or ''}
end
if kind == 'string' then
    size = redis.call('STRLEN', key)
    value = redis.call('GETRANGE', key, 0, 65535)
elseif kind == 'hash' or kind == 'set' then
    local result = redis.call(kind == 'hash' and 'HSCAN' or 'SSCAN', key, start, 'COUNT', page)
    size = redis.call(kind == 'hash' and 'HLEN' or 'SCARD', key)
    next_cursor = result[1]
    local step = kind == 'hash' and 2 or 1
    local count = #result[2] / step
    if count > 1024 then return redis.error_reply('VIEWLIMIT 集合页包含过多成员') end
    local limit = math.min(8192, math.floor(262144 / math.max(count, 1) / 2))
    for i = 1, #result[2], step do
        if kind == 'hash' then
            table.insert(entries, row(result[2][i], result[2][i+1], '', limit))
        else
            table.insert(entries, row('', result[2][i], '', limit))
        end
    end
elseif kind == 'list' or kind == 'zset' then
    local offset = tonumber(start)
    if not offset or offset > 4294967295 then return redis.error_reply('VIEWLIMIT 集合偏移超出范围') end
    size = redis.call(kind == 'list' and 'LLEN' or 'ZCARD', key)
    local last = offset + page - 1
    if last + 1 < size then next_cursor = tostring(last+1) end
    if kind == 'list' then
        for _, data in ipairs(redis.call('LRANGE', key, offset, last)) do
            table.insert(entries, row('', data, '', 8192))
        end
    else
        local values = redis.call('ZRANGE', key, offset, last, 'WITHSCORES')
        for i = 1, #values, 2 do
            table.insert(entries, row('', values[i], values[i+1], 8192))
        end
    end
end
return {kind, ttl, size, next_cursor, value, entries}
