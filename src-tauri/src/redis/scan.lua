local count = tonumber(ARGV[4]) or 96
local kind_filter = ARGV[3]
while true do
    local request = redis.call('SCAN', ARGV[1], 'MATCH', ARGV[2], 'COUNT', count)
    local budget = 262144
    local keys = {}
    for _, key in ipairs(request[2]) do
        local kind = redis.call('TYPE', key).ok
        if kind ~= 'none' and (kind_filter == '' or kind_filter == kind) then
            local preview = string.sub(key, 1, 8192)
            budget = budget - #preview - 48
            if budget < 0 then break end
            table.insert(keys, {preview, #key, kind})
        end
    end

    if budget >= 0 then return {request[1], keys} end
    if count == 1 then return redis.error_reply('VIEWLIMIT 请缩小键名搜索范围') end
    -- Retry from the same cursor; advancing it would skip the rest of this batch.
    count = math.max(1, math.floor(count / 2))
end
