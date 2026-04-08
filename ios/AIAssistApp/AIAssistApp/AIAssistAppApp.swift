//
//  AIAssistAppApp.swift
//  AIAssistApp
//
//  Created by eloqjava on 2/15/26.
//

import SwiftUI
import AIAssistClientLib

@main
struct AIAssistAppApp: App {
    @AppStorage("ai_assist_onboarding_complete") private var onboarded = false

    var body: some Scene {
        WindowGroup {
            if onboarded {
                AIAssistClientLib.MainTabView()
            } else {
                AIAssistClientLib.OnboardingView()
            }
        }
    }
}
