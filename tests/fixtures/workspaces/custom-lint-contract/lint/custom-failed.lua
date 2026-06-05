function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "fail_variable",
  })
end

function fail_variable(ctx)
  if ctx.target.id == "custom-failed" then
    error("script failed for " .. ctx.target.id)
  end
  return {}
end
