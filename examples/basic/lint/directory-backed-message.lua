function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value",
    rule = "consumer-experience/message-not-empty",
    handler = "check_message",
  })
end

function check_message(ctx)
  if ctx.target.variable.id == "directory-backed-message"
      and ctx.target.value == "" then
    return {
      {
        message = "value " .. ctx.target.name .. " must not be empty"
      }
    }
  end
  return {}
end
