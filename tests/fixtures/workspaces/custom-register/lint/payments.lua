function register(lint)
  lint:on({
    stage = "value",
    entity = "variable",
    field = "resolve",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "check_token_budget",
  })

  lint:on({
    stage = "parse",
    entity = "variable",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "check_token_budget",
  })

  lint:on({
    stage = "value",
    entity = "variable",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "missing_handler",
  })
end

function check_token_budget(ctx)
  local default = ctx.target.toml.resolve and ctx.target.toml.resolve.default
  local budget = default
  if type(default) == "table" then
    budget = default[1]
  end
  if budget ~= nil and budget > 5000 then
    return {
      {
        message = ctx.target.id .. ".default"
          .. " exceeds 5000 output tokens"
      }
    }
  end
  return {}
end
