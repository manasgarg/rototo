function register(lint)
  lint:on({
    stage = "value",
    entity = "variable",
    field = "resolve",
    rule = {
          id = "fixture/custom-value-rejected",
          title = "Custom value lint rejected a value",
          help = "Change the fixture value or the Lua lint rule.",
        },
    handler = "reject_value",
  })
end

function reject_value(ctx)
  if ctx.target.id == "custom-value-lint" then
    return {
      {
        message = "custom value lint rejected "
          .. ctx.target.id .. ".default"
      }
    }
  end
  return {}
end
