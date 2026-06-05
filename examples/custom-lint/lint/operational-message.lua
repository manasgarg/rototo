function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value",
    rule = {
          id = "operations/message-not-empty",
          title = "Operational message is empty",
          help = "Set a non-empty message before releasing the workspace.",
        },
    handler = "check_message",
  })
end

function check_message(ctx)
  if ctx.target.variable.id == "operational-message" and ctx.target.value == "" then
    return {
      {
        message = "operational-message value " .. ctx.target.name .. " must not be empty"
      }
    }
  end
  return {}
end
