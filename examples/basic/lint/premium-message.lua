function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value",
    rule = {
          id = "consumer-experience/message-not-empty",
          title = "Directory-backed message is empty",
          help = "Set a non-empty message.",
        },
    handler = "check_message",
  })
end

function check_message(ctx)
  if ctx.target.variable.id == "premium-message"
      and ctx.target.value == "" then
    return {
      {
        message = "value " .. ctx.target.name .. " must not be empty"
      }
    }
  end
  return {}
end
