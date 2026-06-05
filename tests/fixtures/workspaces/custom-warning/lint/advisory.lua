function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    field = "type",
    rule = {
          id = "policy/advisory",
          title = "Workspace policy advisory",
          help = "Review the advisory before release.",
          severity = "warning",
        },
    handler = "check_variable",
  })
end

function check_variable(ctx)
  if ctx.target.id == "message" then
    return {
      { message = "message variable has an advisory" },
    }
  end
  return {}
end
