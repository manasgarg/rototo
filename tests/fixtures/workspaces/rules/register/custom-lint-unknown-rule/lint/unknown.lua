function register(lint)
  lint:on({
    stage = "project",
    entity = "workspace",
    rule = {
          id = "payments/missing-rule",
          title = "payments/missing-rule",
          help = "payments/missing-rule",
        },
    handler = "check",
  })
end

function check(ctx)
  return {}
end
