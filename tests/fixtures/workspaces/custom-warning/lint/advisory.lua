function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    field = "type",
    rule = "policy/advisory",
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
